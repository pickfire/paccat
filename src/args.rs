use clap::{Clap, AppSettings};

const ABOUT: &str = "Print pacman package files";

const TEMPLATE: &str = "
usage: {usage}

a target can be specified as:
    <pkgname>, <repo>/<pkgname>, <url> or <file>.

files can be specified as just the filename or the full path.

{about}

{unified}";

#[derive(Clap, Debug)]
#[clap(about = ABOUT,
    help_template = TEMPLATE,
    version = concat!("v", clap::crate_version!()),
    setting(AppSettings::AllArgsOverrideSelf),
    setting(AppSettings::UnifiedHelpMessage),
    setting(AppSettings::ArgRequiredElseHelp),
)]
pub struct Args {
    #[clap(short = 'x', long, about = "Enable searching using regular expressions")]
    pub regex: bool,
    #[clap(short, long, about = "Print file names instead of file content")]
    pub quiet: bool,
    #[clap(long, about = "Print binary files")]
    pub binary: bool,

    #[clap(short, long, value_name = "path", about = "Set an alternative root directory")]
    pub root: Option<String>,
    #[clap(short = 'b', long, value_name = "path", about = "Set an alternative database location")]
    pub dbpath: Option<String>,
    #[clap(long, value_name = "file", about = "Use an alternative pacman.conf")]
    pub config: Option<String>,
    #[clap(long, value_name = "path", about = "Set an alternative cache directory")]
    pub cachedir: Option<String>,

    #[clap(required = true, value_name = "target", about = "List of packages, package files, or package urls")]
    pub targets: Vec<String>,
    #[clap(required = true, raw = true, value_name = "file", about = "Files to search for")]
    pub files: Vec<String>,
}