//! `OneNote Viewer` desktop application composition root.

#![forbid(unsafe_code)]

mod app;
mod navigation;
mod worker;
mod workspace;

use anyhow::Result;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;

const LICENSE: &str = include_str!("../../../LICENSE");
const SOURCE_CODE: &str = include_str!("../../../SOURCE-CODE.md");
const THIRD_PARTY_NOTICES: &str = include_str!("../../../THIRD-PARTY-NOTICES.md");
const THIRD_PARTY_LICENSES: &str = include_str!("../../../THIRD-PARTY-LICENSES.html");
const SOURCE_REVISION: &str = env!("ONENOTE_VIEWER_SOURCE_REVISION");

enum Invocation {
    Run(Vec<PathBuf>),
    Version,
    License,
    Source,
    ThirdPartyNotices,
    ThirdPartyLicenses,
    CheckIcons,
}

fn main() -> Result<()> {
    match invocation(std::env::args_os().skip(1).collect()) {
        Invocation::Run(requested_sources) => app::run(requested_sources),
        Invocation::Version => write_stdout(&[concat!(
            "OneNote Viewer ",
            env!("CARGO_PKG_VERSION"),
            "\nLicense: GPL-3.0-or-later\n"
        )]),
        Invocation::License => write_stdout(&[LICENSE]),
        Invocation::Source => write_stdout(&[
            &format!(
                "OneNote Viewer {} source revision: {}\n",
                env!("CARGO_PKG_VERSION"),
                SOURCE_REVISION
            ),
            SOURCE_CODE,
        ]),
        Invocation::ThirdPartyNotices => write_stdout(&[THIRD_PARTY_NOTICES]),
        Invocation::ThirdPartyLicenses => write_stdout(&[THIRD_PARTY_LICENSES]),
        Invocation::CheckIcons => app::check_icons(),
    }
}

fn invocation(arguments: Vec<OsString>) -> Invocation {
    if arguments.len() != 1 {
        return Invocation::Run(arguments.into_iter().map(PathBuf::from).collect());
    }

    match arguments[0].to_str() {
        Some("--version") => Invocation::Version,
        Some("--license") => Invocation::License,
        Some("--source") => Invocation::Source,
        Some("--third-party-notices") => Invocation::ThirdPartyNotices,
        Some("--third-party-licenses") => Invocation::ThirdPartyLicenses,
        Some("--check-icons") => Invocation::CheckIcons,
        _ => Invocation::Run(arguments.into_iter().map(PathBuf::from).collect()),
    }
}

fn write_stdout(parts: &[&str]) -> Result<()> {
    let mut stdout = io::stdout().lock();
    for part in parts {
        if let Err(error) = stdout.write_all(part.as_bytes()) {
            if error.kind() == io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(error.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{invocation, Invocation};
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn recognizes_legal_information_commands() {
        assert!(matches!(
            invocation(vec![OsString::from("--license")]),
            Invocation::License
        ));
        assert!(matches!(
            invocation(vec![OsString::from("--source")]),
            Invocation::Source
        ));
        assert!(matches!(
            invocation(vec![OsString::from("--third-party-licenses")]),
            Invocation::ThirdPartyLicenses
        ));
        assert!(matches!(
            invocation(vec![OsString::from("--check-icons")]),
            Invocation::CheckIcons
        ));
    }

    #[test]
    fn preserves_normal_source_arguments() {
        let arguments = vec![OsString::from("onepkg"), OsString::from("section.one")];
        let Invocation::Run(paths) = invocation(arguments) else {
            panic!("source arguments must run the viewer");
        };
        assert_eq!(
            paths,
            vec![PathBuf::from("onepkg"), PathBuf::from("section.one")]
        );
    }
}
