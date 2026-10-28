//! Product-build qualification for the stored-entity reader.
//!
//! CAXA writes a product build such as `16.4.0.12833` into `/*FileInfoStream`.
//! The stored-entity layouts were verified against native samples of five
//! builds. Field production files carry many other build numbers of the same
//! or neighbouring majors (12.5, 13.0, 15.0.x, 16.0, 16.2, 16.4.x, 17.0, 17.1,
//! 18.0.x). The decoders branch on the major number only, so an unverified
//! build inside the qualified major range is read with the rules of its major
//! and reported with an `unverified_product_build` diagnostic instead of being
//! refused. Builds outside the range stay unsupported: there is no sample to
//! anchor their rules.

/// Builds whose stored-entity layouts were verified against native samples.
pub const VERIFIED_PRODUCT_BUILDS: &[&str] = &[
    "10.0.0.0000",
    "11.7.0.5203",
    "15.0.0.5681",
    "16.4.0.13104",
    "18.0.0.18159",
];

/// Inclusive range of product majors read with the verified rules.
pub const QUALIFIED_PRODUCT_MAJORS: (u32, u32) = (10, 18);

/// The leading number of a dotted numeric product build (`16.4.0.12833` → 16).
/// Returns `None` unless the build has two to four all-digit components.
pub fn product_major(build: &str) -> Option<u32> {
    let parts: Vec<&str> = build.split('.').collect();
    if !(2..=4).contains(&parts.len())
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    parts[0].parse().ok()
}

/// The major of a build inside the qualified range, otherwise `None`.
pub fn qualified_major(build: &str) -> Option<u32> {
    let major = product_major(build)?;
    let (low, high) = QUALIFIED_PRODUCT_MAJORS;
    (low..=high).contains(&major).then_some(major)
}

/// Whether the build itself was verified against a native sample.
pub fn is_verified_build(build: &str) -> bool {
    VERIFIED_PRODUCT_BUILDS.contains(&build)
}

/// Verified builds whose rules apply to `major` (same major, or the nearest
/// verified neighbours when the major itself has no sample).
pub fn verified_neighbours(major: u32) -> Vec<&'static str> {
    let same: Vec<&'static str> = VERIFIED_PRODUCT_BUILDS
        .iter()
        .copied()
        .filter(|b| product_major(b) == Some(major))
        .collect();
    if !same.is_empty() {
        return same;
    }
    let below = VERIFIED_PRODUCT_BUILDS
        .iter()
        .copied()
        .rfind(|b| product_major(b).is_some_and(|m| m < major));
    let above = VERIFIED_PRODUCT_BUILDS
        .iter()
        .copied()
        .find(|b| product_major(b).is_some_and(|m| m > major));
    below.into_iter().chain(above).collect()
}

/// Message for the `unverified_product_build` diagnostic, or `None` for a
/// verified build.
pub fn unverified_build_note(build: &str, major: u32) -> Option<String> {
    if is_verified_build(build) {
        return None;
    }
    Some(format!(
        "Product build {build} was not verified against a native sample; stored entities were \
         read with the rules of major {major} (verified: {}).",
        verified_neighbours(major).join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn majors_parse_only_dotted_numeric_builds() {
        assert_eq!(product_major("16.4.0.12833"), Some(16));
        assert_eq!(product_major("10.0"), Some(10));
        assert_eq!(product_major("27.0.0.21423"), Some(27));
        for build in [
            "10",
            "18.0.0.18159 ",
            "Synthetic Build",
            "1.2.3.4.5",
            "16..0.1",
            "",
        ] {
            assert_eq!(product_major(build), None, "{build:?}");
        }
    }

    #[test]
    fn qualified_range_is_closed_and_verified_builds_are_inside() {
        for build in VERIFIED_PRODUCT_BUILDS {
            assert!(is_verified_build(build));
            assert!(qualified_major(build).is_some(), "{build}");
        }
        assert_eq!(qualified_major("12.5.0.5302"), Some(12));
        assert_eq!(qualified_major("17.1.0.15581"), Some(17));
        assert_eq!(qualified_major("9.9.0.1"), None);
        assert_eq!(qualified_major("19.0.0.1"), None);
        assert_eq!(qualified_major("27.0.0.21423"), None);
    }

    #[test]
    fn neighbours_prefer_the_same_major() {
        assert_eq!(verified_neighbours(16), vec!["16.4.0.13104"]);
        assert_eq!(
            verified_neighbours(17),
            vec!["16.4.0.13104", "18.0.0.18159"]
        );
        assert_eq!(verified_neighbours(12), vec!["11.7.0.5203", "15.0.0.5681"]);
        assert_eq!(unverified_build_note("18.0.0.18159", 18), None);
        let note = unverified_build_note("18.0.0.17670", 18).unwrap();
        assert!(note.contains("major 18") && note.contains("18.0.0.18159"));
    }
}
