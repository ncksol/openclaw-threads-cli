use crate::error::{CliError, ErrorCategory};

pub fn validate_post_text(text: &str) -> Result<(), CliError> {
    if text.trim().is_empty() {
        return Err(CliError::new(
            ErrorCategory::Validation,
            "text must be non-empty",
        ));
    }
    Ok(())
}

pub fn validate_reply_to(reply_to: &str) -> Result<(), CliError> {
    if reply_to.trim().is_empty() {
        return Err(CliError::new(
            ErrorCategory::Validation,
            "reply-to post id must be non-empty",
        ));
    }
    Ok(())
}

pub fn validate_topic_tag(tag: &str) -> Result<(), CliError> {
    let value = tag.trim();
    if value.is_empty() {
        return Err(CliError::new(
            ErrorCategory::Validation,
            "tag must be non-empty",
        ));
    }
    if value.chars().count() > 50 {
        return Err(CliError::new(
            ErrorCategory::Validation,
            "tag must be <= 50 characters",
        ));
    }
    if value
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
    {
        return Err(CliError::new(
            ErrorCategory::Validation,
            "tag contains unsupported characters",
        ));
    }
    Ok(())
}

pub fn validate_source_url(url: &str) -> Result<(), CliError> {
    let parsed = url::Url::parse(url).map_err(|_| {
        CliError::new(
            ErrorCategory::Validation,
            "source link must be a valid absolute URL",
        )
    })?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(CliError::new(
            ErrorCategory::Validation,
            "source link must use http or https",
        ));
    }
    Ok(())
}
