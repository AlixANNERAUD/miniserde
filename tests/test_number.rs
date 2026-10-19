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
fn test_de() {
    let cases = [
        "0.1",
        "7.4e47",
        "5.92e48",
        "7.35e66",
        "8.32116e55",
        "1.7976931348623157e308",
        "2.2250738585072014e-308",
        "4.9406564584124654e-324",
        "1.00000000000000011102230246251565404236316680908203125",
        "123456789012345678901234567890",
        "-1e-400",
        "1e-99999999999",
    ];

    for case in cases {
        let expected = case.parse::<f64>().unwrap();
        let actual: f64 = json::from_str(case).unwrap();
        assert_eq!(actual.to_bits(), expected.to_bits(), "{case}");
    }

    for case in ["1e309", "-1e309", "1e99999999999", "1.", "1e", "1e+", ".1", "01", "-"] {
        assert!(json::from_str::<f64>(case).is_err(), "{case}");
    }
}
