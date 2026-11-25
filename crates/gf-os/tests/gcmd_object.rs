//! Integration test for `#[derive(CommandObject)]` (ADR-108).

use std::sync::{Mutex, OnceLock};

use gf_cmd_derive::CommandObject;
use gf_os::gcmd::{CommandError, new_from_object};

fn recorded_state() -> &'static Mutex<Option<(u16, bool, String)>> {
    static RECORDED: OnceLock<Mutex<Option<(u16, bool, String)>>> = OnceLock::new();
    RECORDED.get_or_init(|| Mutex::new(None))
}

#[derive(CommandObject)]
#[cmd(
    name = "serve",
    brief = "serve the application",
    usage = "app serve [OPTION]",
    description = "Starts the HTTP server"
)]
struct Serve {
    #[cmd(short = "p", brief = "server port", default = "8080")]
    port: u16,
    #[cmd(short = "v", brief = "verbose output", orphan)]
    verbose: bool,
    #[cmd(rename = "target", brief = "target file")]
    target: String,
    #[cmd(skip)]
    internal: u8,
}

impl Serve {
    fn run(&self) -> Result<(), CommandError> {
        let _ = self.internal;
        *recorded_state().lock().unwrap() = Some((self.port, self.verbose, self.target.clone()));
        Ok(())
    }
}

#[test]
fn derived_metadata_is_exposed() {
    use gf_os::gcmd::CommandObject;

    assert_eq!(Serve::command_name(), "serve");
    let meta = Serve::command_meta();
    assert_eq!(meta.brief, "serve the application");
    assert_eq!(meta.usage, "app serve [OPTION]");
    assert_eq!(meta.description, "Starts the HTTP server");
    assert_eq!(meta.arguments.len(), 3);
    assert_eq!(meta.arguments[0].name, "port");
    assert_eq!(meta.arguments[0].short, "p");
    assert_eq!(meta.arguments[0].default, "8080");
    assert!(!meta.arguments[0].is_arg);
    assert!(meta.arguments[1].orphan);
    assert_eq!(meta.arguments[2].name, "target");
}

#[test]
fn object_builds_a_command_and_runs_it() {
    let command = new_from_object::<Serve>().expect("command");
    assert_eq!(command.name, "serve");
    assert_eq!(command.brief, "serve the application");
    assert_eq!(command.arguments.len(), 3);

    let _ = command.run_with_error(&[
        "serve".to_owned(),
        "-p".to_owned(),
        "9090".to_owned(),
        "-v".to_owned(),
        "--target".to_owned(),
        "main.go".to_owned(),
    ]);
    let recorded = recorded_state().lock().unwrap().clone();
    assert_eq!(
        recorded,
        Some((9090, true, "main.go".to_owned())),
        "the derived parser should fill every field"
    );

    // Defaults apply when the option is omitted.
    let _ = command.run_with_error(&["serve".to_owned(), "-v".to_owned()]);
    let recorded = recorded_state().lock().unwrap().clone();
    assert_eq!(
        recorded,
        Some((8080, true, String::new())),
        "the default port and empty target should be filled"
    );
}
