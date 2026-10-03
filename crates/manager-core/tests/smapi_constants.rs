//! The game range is read from SMAPI's own Constants.cs, as published at
//! each release tag (fixtures copied from the SMAPI repository).

use manager_core::smapi::catalog::parse_constants;

fn fixture(tag: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/smapi-constants-{tag}.cs",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn real_smapi_constants_give_the_declared_game_range() {
    assert_eq!(
        parse_constants(&fixture("4.5.2")),
        (Some("1.6.14".into()), None)
    );
    assert_eq!(
        parse_constants(&fixture("4.1.10")),
        (Some("1.6.14".into()), None)
    );
    assert_eq!(
        parse_constants(&fixture("3.18.6")),
        (Some("1.5.6".into()), Some("1.5.6".into()))
    );
}
