//! Input validators run before any destructive action. Each returns the first
//! problem found, described for a human.

use crate::error::{Error, Result};

/// A DNS-label-style name: 1..=`max_len` chars, ASCII-alphanumeric or hyphen,
/// not starting/ending with a hyphen. `kind` labels the error message.
fn validate_dns_label(name: &str, max_len: usize, kind: &str) -> Result<()> {
    let valid = (1..=max_len).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if valid {
        Ok(())
    } else {
        Err(Error::Config(format!("invalid {kind} `{name}`")))
    }
}

/// GitHub usernames: 1–39 chars, alphanumeric or single hyphens, not
/// starting/ending with a hyphen. Validated so the `.keys` URL is well-formed.
pub(super) fn validate_github_user(name: &str) -> Result<()> {
    validate_dns_label(name, 39, "GitHub username")
}

/// Package names: non-empty, and limited to pacman's allowed characters so a
/// stray token cannot only blow up mid-`pacstrap` after the disk is wiped.
pub(super) fn validate_package_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '+' | '@'));
    if valid {
        Ok(())
    } else {
        Err(Error::Config(format!("invalid package name `{name}`")))
    }
}

/// Hostnames: 1–63 chars, alphanumeric or hyphen, not starting/ending with a hyphen.
pub(super) fn validate_hostname(name: &str) -> Result<()> {
    validate_dns_label(name, 63, "hostname")
}

/// Linux usernames: start with a lowercase letter or underscore, followed by
/// lowercase letters, digits, underscores or hyphens; at most 32 chars.
pub(super) fn validate_username(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let head_ok = matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_');
    let tail_ok =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if head_ok && tail_ok && (1..=32).contains(&name.len()) {
        Ok(())
    } else {
        Err(Error::Config(format!("invalid username `{name}`")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_rules() {
        assert!(validate_hostname("arch").is_ok());
        assert!(validate_hostname("my-arch-01").is_ok());
        assert!(validate_hostname("-bad").is_err());
        assert!(validate_hostname("bad-").is_err());
        assert!(validate_hostname("").is_err());
        assert!(validate_hostname("white space").is_err());
    }

    #[test]
    fn username_rules() {
        assert!(validate_username("arch").is_ok());
        assert!(validate_username("_svc").is_ok());
        assert!(validate_username("1bad").is_err());
        assert!(validate_username("Bad").is_err());
        assert!(validate_username("").is_err());
    }

    #[test]
    fn package_name_rules() {
        assert!(validate_package_name("base-devel").is_ok());
        assert!(validate_package_name("gtk+").is_ok());
        assert!(validate_package_name("lib32-glibc").is_ok());
        assert!(validate_package_name("").is_err());
        assert!(validate_package_name("bad name").is_err());
        assert!(validate_package_name("rm;reboot").is_err());
    }
}
