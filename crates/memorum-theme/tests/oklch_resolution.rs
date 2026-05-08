use memorum_theme::{ColorCapability, OklchColor, ResolvedColor, Resolver};

#[test]
fn parses_oklch_and_hex_and_rejects_malformed_values() {
    let color = OklchColor::parse_oklch("oklch(0.16 0.006 70 / 0.8)").expect("oklch parses");
    assert!((color.l - 0.16).abs() < f32::EPSILON);
    assert!(OklchColor::parse("#ff0000").is_ok());
    assert!(OklchColor::parse("oklch(nope)").is_err());
}

#[test]
fn resolver_produces_stable_output_for_each_capability() {
    let red = OklchColor::parse("#ff0000").expect("red hex parses");
    assert_eq!(
        Resolver::with_capability(ColorCapability::TrueColor).resolve_oklch(&red),
        ResolvedColor::Rgb(255, 0, 0)
    );
    assert_eq!(Resolver::with_capability(ColorCapability::Indexed256).resolve_oklch(&red), ResolvedColor::Indexed(196));
    assert!(matches!(
        Resolver::with_capability(ColorCapability::Indexed16).resolve_oklch(&red),
        ResolvedColor::Named(_)
    ));
    assert!(matches!(
        Resolver::with_capability(ColorCapability::Monochrome).resolve_oklch(&red),
        ResolvedColor::MonochromeBlack | ResolvedColor::MonochromeWhite
    ));
}
