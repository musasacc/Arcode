//! `vars` tests.

use super::*;

/// Fixed time, counting random numbers.
struct Fixed {
    now: u64,
    next: u64,
}

impl Dynamic for Fixed {
    fn now_ms(&mut self) -> u64 {
        self.now
    }
    fn random_u64(&mut self) -> u64 {
        self.next = self.next.wrapping_add(0x9E37_79B9_7F4A_7C15);
        self.next
    }
}

fn fixed() -> Fixed {
    Fixed {
        now: 1_700_000_000_123,
        next: 0,
    }
}

fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn substitutes_nested_variables() {
    let v = vars(&[
        ("host", "localhost"),
        ("port", "8080"),
        ("base", "http://{{host}}:{{ port }}"),
    ]);
    assert_eq!(
        resolve("{{base}}/users/{{port}}", &v, &mut fixed()),
        Ok("http://localhost:8080/users/8080".into())
    );
    assert_eq!(resolve("no vars", &v, &mut fixed()), Ok("no vars".into()));
    assert_eq!(
        resolve("open {{ never closed", &v, &mut fixed()),
        Ok("open {{ never closed".into())
    );
}

#[test]
fn unknown_and_cyclic_variables_are_errors() {
    let v = vars(&[("a", "{{b}}"), ("b", "{{a}}"), ("self", "x{{self}}")]);
    assert_eq!(
        resolve("{{missing}}", &v, &mut fixed()),
        Err(VarError::Unknown("missing".into()))
    );
    assert!(matches!(
        resolve("{{a}}", &v, &mut fixed()),
        Err(VarError::Cycle(_))
    ));
    assert_eq!(
        resolve("{{self}}", &v, &mut fixed()),
        Err(VarError::Cycle("self".into()))
    );
    assert!(
        VarError::Unknown("x".into())
            .message()
            .starts_with("unknown variable {{x}}")
    );
    // A variable used twice side by side is not a cycle.
    let v = vars(&[("x", "1"), ("y", "{{x}}{{x}}")]);
    assert_eq!(resolve("{{y}}{{x}}", &v, &mut fixed()), Ok("111".into()));
}

#[test]
fn timestamps() {
    let v = BTreeMap::new();
    assert_eq!(
        resolve("{{$timestamp}}", &v, &mut fixed()),
        Ok("1700000000".into())
    );
    assert_eq!(
        resolve("{{$isoTimestamp}}", &v, &mut fixed()),
        Ok("2023-11-14T22:13:20.123Z".into())
    );
    assert_eq!(
        resolve("{{$datetime iso8601}}", &v, &mut fixed()),
        Ok("2023-11-14T22:13:20.123Z".into())
    );
    assert_eq!(iso8601(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso8601(951_782_400_000), "2000-02-29T00:00:00.000Z");
    assert_eq!(iso8601(4_107_542_399_999), "2100-02-28T23:59:59.999Z");
}

#[test]
fn uuids_are_v4_and_fresh_per_occurrence() {
    let v = BTreeMap::new();
    let out = resolve("{{$uuid}} {{$random.uuid}} {{$guid}}", &v, &mut fixed()).unwrap();
    let ids: Vec<&str> = out.split(' ').collect();
    assert_eq!(ids.len(), 3);
    for id in &ids {
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12],
            "{id}"
        );
        assert!(parts[2].starts_with('4'), "version 4: {id}");
        assert!("89ab".contains(&parts[3][..1]), "variant: {id}");
        assert!(id.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }
    assert!(ids[0] != ids[1] && ids[1] != ids[2]);
}

#[test]
fn random_integers_stay_in_range() {
    let v = BTreeMap::new();
    let mut d = fixed();
    for _ in 0..200 {
        let n: i64 = resolve("{{$randomInt}}", &v, &mut d)
            .unwrap()
            .parse()
            .unwrap();
        assert!((0..=1000).contains(&n));
        let n: i64 = resolve("{{$randomInt -5 5}}", &v, &mut d)
            .unwrap()
            .parse()
            .unwrap();
        assert!((-5..5).contains(&n));
        let n: i64 = resolve("{{$random.integer(10, 12)}}", &v, &mut d)
            .unwrap()
            .parse()
            .unwrap();
        assert!((10..12).contains(&n));
    }
    for bad in [
        "{{$randomInt 5}}",
        "{{$randomInt 5 5}}",
        "{{$randomInt 9 1}}",
        "{{$random.integer 1, 2}}",
        "{{$uuid x}}",
    ] {
        assert!(
            matches!(resolve(bad, &v, &mut d), Err(VarError::BadArguments(_))),
            "{bad}"
        );
    }
}

#[test]
fn sandbox_only_variables_are_refused() {
    let v = BTreeMap::new();
    assert_eq!(
        resolve("{{$processEnv HOME}}", &v, &mut fixed()),
        Err(VarError::Unsupported("$processEnv".into()))
    );
    assert!(
        VarError::Unsupported("$dotenv".into())
            .message()
            .contains("sandbox")
    );
}

#[test]
fn system_dynamic_gives_different_values() {
    let mut d = SystemDynamic::default();
    assert_ne!(d.random_u64(), d.random_u64());
    assert!(d.now_ms() > 1_600_000_000_000);
}
