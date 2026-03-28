#[path = "../src/error.rs"]
mod error;
#[allow(dead_code)]
#[path = "../src/cli/validation.rs"]
mod validation;

#[test]
fn text_validation_rejects_blank() {
    let err = validation::validate_post_text("   ").expect_err("expected validation fail");
    assert_eq!(err.category.as_code(), "VALIDATION_ERROR");
}

#[test]
fn topic_tag_validation_rejects_bad_chars() {
    let err = validation::validate_topic_tag("bad tag!").expect_err("expected validation fail");
    assert_eq!(err.category.as_code(), "VALIDATION_ERROR");
}

#[test]
fn source_url_validation_requires_http_scheme() {
    let err = validation::validate_source_url("ftp://example.com").expect_err("expected fail");
    assert_eq!(err.category.as_code(), "VALIDATION_ERROR");
}

#[test]
fn source_url_validation_accepts_https() {
    validation::validate_source_url("https://example.com").expect("expected valid url");
}
