use miniserde::json;

#[test]
fn test_ser() {
    let cases = &[
        (1.0, "1.0"),
        (f64::NAN.copysign(1.0), "null"),
        (f64::NAN.copysign(-1.0), "null"),
        (f64::INFINITY, "null"),
        (f64::NEG_INFINITY, "null"),
    ];

    for (number, expected) in cases {
        let actual = json::to_string(number);
        assert_eq!(actual, *expected);
    }
}

#[test]
fn test_f32() {
    let cases = [
        (0.1_f32, "0.1"),
        (1.0e-7, "1e-7"),
        (16_777_216.0, "16777216.0"),
        (f32::MAX, "3.4028235e+38"),
        (f32::MIN_POSITIVE, "1.1754944e-38"),
        (f32::NAN, "null"),
        (f32::INFINITY, "null"),
    ];

    for (number, expected) in cases {
        let actual = json::to_string(&number);
        assert_eq!(actual, expected);
        if number.is_finite() {
            assert_eq!(json::from_str::<f32>(&actual).unwrap(), number);
        }
    }

    assert!(json::from_str::<f32>("1e300").is_err());
    assert!(json::from_str::<f32>("-1e39").is_err());
    assert_eq!(json::from_str::<f32>("1e-300").unwrap(), 0.0);
}
