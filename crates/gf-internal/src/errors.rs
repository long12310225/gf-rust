//! Port of `internal/errors`: selection of the error stack detail mode.

use std::sync::OnceLock;

use crate::command;

const COMMAND_ENV_KEY_FOR_BRIEF: &str = "gf.gerror.brief";
const COMMAND_ENV_KEY_FOR_STACK_MODE: &str = "gf.gerror.stack.mode";

/// How much stack information an error prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackMode {
    /// No framework stack frames.
    Brief,
    /// Detailed stack frames, including framework frames.
    Detail,
}

static STACK_MODE: OnceLock<StackMode> = OnceLock::new();

fn stack_mode() -> StackMode {
    *STACK_MODE.get_or_init(|| {
        let mut mode = StackMode::Brief;
        let brief_setting = command::get_opt_with_env(COMMAND_ENV_KEY_FOR_BRIEF, None);
        if brief_setting == "1" || brief_setting == "true" {
            mode = StackMode::Brief;
        }
        let stack_mode_setting = command::get_opt_with_env(COMMAND_ENV_KEY_FOR_STACK_MODE, None);
        match stack_mode_setting.as_str() {
            "brief" => mode = StackMode::Brief,
            "detail" => mode = StackMode::Detail,
            _ => {}
        }
        mode
    })
}

/// Returns whether the configured stack mode is [`StackMode::Brief`].
pub fn is_stack_mode_brief() -> bool {
    stack_mode() == StackMode::Brief
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_is_the_default() {
        assert!(is_stack_mode_brief());
        assert_eq!(stack_mode(), StackMode::Brief);
    }
}
