use copilot_bridge_atlas_lib::AppType;
use std::str::FromStr;

#[test]
fn accepts_only_codex_with_case_and_whitespace_normalization() {
    for value in ["codex", " CODEX ", "\tcoDeX\n"] {
        assert_eq!(AppType::from_str(value).unwrap(), AppType::Codex);
    }
    assert_eq!(AppType::Codex.as_str(), "codex");
    let error = AppType::from_str("unsupported").unwrap_err().to_string();
    assert!(error.contains("unsupported"));
    assert!(error.contains("codex"));
}
