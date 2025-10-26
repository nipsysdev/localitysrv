use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "localitysrv",
    about = "HTTP server that serves pmtiles for localities worldwide",
    version,
    author
)]
pub struct Args {
    #[arg(short, long)]
    pub non_interactive: bool,

    #[arg(long)]
    pub no_download: bool,

    #[arg(long)]
    pub no_extract: bool,
}

impl Args {
    pub fn should_download_database(&self) -> bool {
        if self.non_interactive && !self.no_download {
            return true;
        }
        false
    }

    pub fn should_extract_localities(&self) -> bool {
        if self.non_interactive && !self.no_extract {
            return true;
        }
        false
    }

    pub fn is_interactive_mode(&self) -> bool {
        !self.non_interactive && !self.no_download && !self.no_extract
    }
}
