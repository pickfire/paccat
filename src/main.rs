use crate::args::Args;
use crate::pacman::{alpm_init, get_download_url, get_dbpkg};
use alpm::{Alpm, Package};
use alpm_utils::DbListExt;
use anyhow::{bail, Context, Result};
use clap::Clap;
use compress_tools::{ArchiveContents, ArchiveIterator};
use nix::sys::signal::{signal, SigHandler, Signal};
use nix::unistd::isatty;
use regex::RegexSet;
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;

mod args;
mod pacman;

#[derive(PartialEq, Eq)]
enum EntryState {
    Skip,
    FirstChunk,
    Reading,
}

struct Match<'a> {
    with: MatchWith<'a>,
    exact_file: bool,
}

impl<'a> Match<'a> {
    fn new(regex: bool, files: &'a [&'a str]) -> Result<Self> {
        let exact_file = files.iter().any(|f| f.contains('/'));
        let with = MatchWith::new(regex, files)?;
        Ok(Self { exact_file, with })
    }

    fn is_match(&self, file: &str) -> bool {
        let file = if !self.exact_file {
            file.rsplit('/').next().unwrap()
        } else {
            file
        };

        if file.is_empty() {
            return false;
        }

        match self.with {
            MatchWith::Regex(ref r) => r.is_match(file),
            MatchWith::Files(f) => f.iter().any(|&t| t == file),
        }
    }
}

enum MatchWith<'a> {
    Regex(RegexSet),
    Files(&'a [&'a str]),
}

impl<'a> MatchWith<'a> {
    fn new(regex: bool, files: &'a [&'a str]) -> Result<Self> {
        let match_with = if regex {
            let regex = RegexSet::new(files)?;
            MatchWith::Regex(regex)
        } else {
            MatchWith::Files(files)
        };

        Ok(match_with)
    }
}

fn main() {
    unsafe { signal(Signal::SIGPIPE, SigHandler::SigDfl).unwrap() };

    match run() {
        Ok(i) => std::process::exit(i),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}

fn run() -> Result<i32> {
    let mut args = args::Args::parse();
    let mut ret = 0;
    let stdout = io::stdout();

    args.binary |= !isatty(stdout.as_raw_fd()).unwrap_or(false);

    let files = args
        .files
        .iter()
        .map(|f| f.trim_start_matches('/'))
        .collect::<Vec<_>>();

    let matcher = Match::new(args.regex, &files)?;
    let alpm = alpm_init(&args)?;

    let pkgs = get_targets(&alpm, &args, &matcher)?;

    for pkg in pkgs {
        let file = File::open(&pkg).with_context(|| format!("failed to open {}", pkg))?;
        let archive = ArchiveIterator::from_read(file)?;
        ret |= dump_files(archive, &matcher, &args)?;
    }

    Ok(ret)
}

fn dump_files<R>(archive: ArchiveIterator<R>, matcher: &Match, args: &Args) -> Result<i32>
where
    R: Read + Seek,
{
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut state = EntryState::Skip;
    let mut found = 0;
    let mut cur_file = String::new();

    for content in archive {
        match content {
            ArchiveContents::StartOfEntry(file) => {
                if matcher.is_match(&file) {
                    found += 1;
                    if args.quiet {
                        writeln!(stdout, "{}", file)?;
                    } else {
                        cur_file = file;
                        state = EntryState::FirstChunk;
                    }
                }
            }
            ArchiveContents::DataChunk(v) if state == EntryState::FirstChunk => {
                if is_binary(&v) && !args.binary {
                    state = EntryState::Skip;
                    eprintln!("{} is a binary file -- use --binary to print", cur_file)
                } else {
                    stdout.write_all(&v)?
                }
            }
            ArchiveContents::DataChunk(v) if state == EntryState::Reading => {
                stdout.write_all(&v)?
            }
            ArchiveContents::DataChunk(_) => (),
            ArchiveContents::EndOfEntry => state = EntryState::Skip,
            ArchiveContents::Err(e) => {
                return Err(e.into());
            }
        }
    }

    let ret = match matcher.with {
        MatchWith::Files(f) if f.len() as i32 == found => 0,
        MatchWith::Regex(_) if found != 0 => 0,
        _ => 1,
    };

    Ok(ret)
}

fn is_binary(data: &[u8]) -> bool {
    data.iter().take(512).any(|&b| b == 0)
}

fn get_targets(alpm: &Alpm, args: &Args, matcher: &Match) -> Result<Vec<String>> {
    let mut download = Vec::new();
    let mut repo = Vec::new();
    let mut files = Vec::new();
    let dbs = alpm.syncdbs();

    if args.targets.is_empty() {
        if args.localdb {
            let pkgs = alpm.localdb().pkgs();
            let pkgs = pkgs
                .iter()
                .filter(|pkg| want_pkg(alpm, *pkg, matcher))
                .filter_map(|p| dbs.pkg(p.name()).ok());
            repo.extend(pkgs);
        } else {
            let pkgs = dbs
                .iter()
                .flat_map(|db| db.pkgs())
                .filter(|pkg| want_pkg(alpm, *pkg, matcher));
            repo.extend(pkgs);
        }
    } else {
        for targ in &args.targets {
            if let Ok(pkg) = get_dbpkg(alpm, targ) {
                if want_pkg(alpm, pkg, matcher) {
                    repo.push(pkg);
                }
            } else if targ.contains("://") {
                download.push(targ.clone());
            } else if Path::new(&targ).exists() {
                files.push(targ.to_string());
            } else {
                bail!("'{}' is not a package, file or url", targ);
            }
        }
    }

    // todo filter repopkg files

    for pkg in repo {
        download.push(get_download_url(pkg)?);
    }

    let downloaded = alpm.fetch_pkgurl(download.into_iter())?;
    files.extend(downloaded);

    Ok(files)
}

fn want_pkg(_alpm: &Alpm, pkg: Package, matcher: &Match) -> bool {
    let files = pkg.files();
    files.files().iter().any(|f| matcher.is_match(f.name()))
}
