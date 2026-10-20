//! Independently rounded trig samples, including signed axes, large reduction and degree boundaries.
//! Decimal series at 600/900 digits agree before direct IEEE rounding (Plan235).
#[derive(Clone, Copy)]
pub struct Sample {
    pub operation: &'static str,
    pub bits: u8,
    pub input: u64,
    pub other: u64,
    pub expected: Option<u64>,
}
#[rustfmt::skip]
pub const SAMPLES: &[Sample] = &[
    Sample { operation: "sin", bits: 32, input: 0x0, other: 0x0, expected: Some(0x0) },
    Sample { operation: "sin", bits: 32, input: 0x80000000, other: 0x0, expected: Some(0x80000000) },
    Sample { operation: "sin", bits: 32, input: 0x3f800000, other: 0x0, expected: Some(0x3f576aa4) },
    Sample { operation: "sin", bits: 32, input: 0xbf800000, other: 0x0, expected: Some(0xbf576aa4) },
    Sample { operation: "sin", bits: 32, input: 0x322bcc77, other: 0x0, expected: Some(0x322bcc77) },
    Sample { operation: "sin", bits: 32, input: 0xb22bcc77, other: 0x0, expected: Some(0xb22bcc77) },
    Sample { operation: "sin", bits: 32, input: 0x40490fdb, other: 0x0, expected: Some(0xb3bbbd2e) },
    Sample { operation: "sin", bits: 32, input: 0xc0490fdb, other: 0x0, expected: Some(0x33bbbd2e) },
    Sample { operation: "sin", bits: 32, input: 0x3fc90fdb, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "sin", bits: 32, input: 0x501502f9, other: 0x0, expected: Some(0xbef99a64) },
    Sample { operation: "sin", bits: 32, input: 0x60ad78ec, other: 0x0, expected: Some(0x3f281569) },
    Sample { operation: "sin", bits: 32, input: 0x7f800000, other: 0x0, expected: None },
    Sample { operation: "sin", bits: 32, input: 0xff800000, other: 0x0, expected: None },
    Sample { operation: "sin", bits: 32, input: 0x7fc00000, other: 0x0, expected: None },
    Sample { operation: "sin", bits: 32, input: 0x1, other: 0x0, expected: Some(0x1) },
    Sample { operation: "sin", bits: 32, input: 0x80000001, other: 0x0, expected: Some(0x80000001) },
    Sample { operation: "sin", bits: 32, input: 0x800000, other: 0x0, expected: Some(0x800000) },
    Sample { operation: "sin", bits: 32, input: 0x7f7fffff, other: 0x0, expected: Some(0xbf0599b3) },
    Sample { operation: "sin", bits: 32, input: 0xff7fffff, other: 0x0, expected: Some(0x3f0599b3) },
    Sample { operation: "sin", bits: 32, input: 0x40490fda, other: 0x0, expected: Some(0x34222169) },
    Sample { operation: "sin", bits: 32, input: 0x40490fdc, other: 0x0, expected: Some(0xb4aeef4c) },
    Sample { operation: "sin", bits: 32, input: 0x3fc90fda, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "sin", bits: 32, input: 0x3fc90fdc, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "cos", bits: 32, input: 0x0, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "cos", bits: 32, input: 0x80000000, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "cos", bits: 32, input: 0x3f800000, other: 0x0, expected: Some(0x3f0a5140) },
    Sample { operation: "cos", bits: 32, input: 0xbf800000, other: 0x0, expected: Some(0x3f0a5140) },
    Sample { operation: "cos", bits: 32, input: 0x322bcc77, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "cos", bits: 32, input: 0xb22bcc77, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "cos", bits: 32, input: 0x40490fdb, other: 0x0, expected: Some(0xbf800000) },
    Sample { operation: "cos", bits: 32, input: 0xc0490fdb, other: 0x0, expected: Some(0xbf800000) },
    Sample { operation: "cos", bits: 32, input: 0x3fc90fdb, other: 0x0, expected: Some(0xb33bbd2e) },
    Sample { operation: "cos", bits: 32, input: 0x501502f9, other: 0x0, expected: Some(0x3f5f84c5) },
    Sample { operation: "cos", bits: 32, input: 0x60ad78ec, other: 0x0, expected: Some(0x3f411723) },
    Sample { operation: "cos", bits: 32, input: 0x7f800000, other: 0x0, expected: None },
    Sample { operation: "cos", bits: 32, input: 0xff800000, other: 0x0, expected: None },
    Sample { operation: "cos", bits: 32, input: 0x7fc00000, other: 0x0, expected: None },
    Sample { operation: "cos", bits: 32, input: 0x1, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "cos", bits: 32, input: 0x80000001, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "cos", bits: 32, input: 0x800000, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "cos", bits: 32, input: 0x7f7fffff, other: 0x0, expected: Some(0x3f5a5f96) },
    Sample { operation: "cos", bits: 32, input: 0xff7fffff, other: 0x0, expected: Some(0x3f5a5f96) },
    Sample { operation: "cos", bits: 32, input: 0x40490fda, other: 0x0, expected: Some(0xbf800000) },
    Sample { operation: "cos", bits: 32, input: 0x40490fdc, other: 0x0, expected: Some(0xbf800000) },
    Sample { operation: "cos", bits: 32, input: 0x3fc90fda, other: 0x0, expected: Some(0x33a22169) },
    Sample { operation: "cos", bits: 32, input: 0x3fc90fdc, other: 0x0, expected: Some(0xb42eef4c) },
    Sample { operation: "atan2", bits: 32, input: 0x0, other: 0x0, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 32, input: 0x0, other: 0x80000000, expected: Some(0x40490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x0, other: 0x3f800000, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 32, input: 0x0, other: 0xbf800000, expected: Some(0x40490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x0, other: 0x7f800000, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 32, input: 0x0, other: 0xff800000, expected: Some(0x40490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x0, other: 0x7fc00000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x80000000, other: 0x0, expected: Some(0x80000000) },
    Sample { operation: "atan2", bits: 32, input: 0x80000000, other: 0x80000000, expected: Some(0xc0490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x80000000, other: 0x3f800000, expected: Some(0x80000000) },
    Sample { operation: "atan2", bits: 32, input: 0x80000000, other: 0xbf800000, expected: Some(0xc0490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x80000000, other: 0x7f800000, expected: Some(0x80000000) },
    Sample { operation: "atan2", bits: 32, input: 0x80000000, other: 0xff800000, expected: Some(0xc0490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x80000000, other: 0x7fc00000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x3f800000, other: 0x0, expected: Some(0x3fc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x3f800000, other: 0x80000000, expected: Some(0x3fc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x3f800000, other: 0x3f800000, expected: Some(0x3f490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x3f800000, other: 0xbf800000, expected: Some(0x4016cbe4) },
    Sample { operation: "atan2", bits: 32, input: 0x3f800000, other: 0x7f800000, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 32, input: 0x3f800000, other: 0xff800000, expected: Some(0x40490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x3f800000, other: 0x7fc00000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0xbf800000, other: 0x0, expected: Some(0xbfc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xbf800000, other: 0x80000000, expected: Some(0xbfc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xbf800000, other: 0x3f800000, expected: Some(0xbf490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xbf800000, other: 0xbf800000, expected: Some(0xc016cbe4) },
    Sample { operation: "atan2", bits: 32, input: 0xbf800000, other: 0x7f800000, expected: Some(0x80000000) },
    Sample { operation: "atan2", bits: 32, input: 0xbf800000, other: 0xff800000, expected: Some(0xc0490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xbf800000, other: 0x7fc00000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x7f800000, other: 0x0, expected: Some(0x3fc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x7f800000, other: 0x80000000, expected: Some(0x3fc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x7f800000, other: 0x3f800000, expected: Some(0x3fc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x7f800000, other: 0xbf800000, expected: Some(0x3fc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x7f800000, other: 0x7f800000, expected: Some(0x3f490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x7f800000, other: 0xff800000, expected: Some(0x4016cbe4) },
    Sample { operation: "atan2", bits: 32, input: 0x7f800000, other: 0x7fc00000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0xff800000, other: 0x0, expected: Some(0xbfc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xff800000, other: 0x80000000, expected: Some(0xbfc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xff800000, other: 0x3f800000, expected: Some(0xbfc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xff800000, other: 0xbf800000, expected: Some(0xbfc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xff800000, other: 0x7f800000, expected: Some(0xbf490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xff800000, other: 0xff800000, expected: Some(0xc016cbe4) },
    Sample { operation: "atan2", bits: 32, input: 0xff800000, other: 0x7fc00000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x7fc00000, other: 0x0, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x7fc00000, other: 0x80000000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x7fc00000, other: 0x3f800000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x7fc00000, other: 0xbf800000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x7fc00000, other: 0x7f800000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x7fc00000, other: 0xff800000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x7fc00000, other: 0x7fc00000, expected: None },
    Sample { operation: "atan2", bits: 32, input: 0x1, other: 0x7f7fffff, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 32, input: 0x7f7fffff, other: 0x1, expected: Some(0x3fc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x800000, other: 0x7f7fffff, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 32, input: 0x7f7fffff, other: 0x800000, expected: Some(0x3fc90fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x3f800000, other: 0xc0000000, expected: Some(0x402b6374) },
    Sample { operation: "atan2", bits: 32, input: 0xbf800000, other: 0x40000000, expected: Some(0xbeed6338) },
    Sample { operation: "sin", bits: 64, input: 0x0, other: 0x0, expected: Some(0x0) },
    Sample { operation: "sin", bits: 64, input: 0x8000000000000000, other: 0x0, expected: Some(0x8000000000000000) },
    Sample { operation: "sin", bits: 64, input: 0x3ff0000000000000, other: 0x0, expected: Some(0x3feaed548f090cee) },
    Sample { operation: "sin", bits: 64, input: 0xbff0000000000000, other: 0x0, expected: Some(0xbfeaed548f090cee) },
    Sample { operation: "sin", bits: 64, input: 0x3e45798ee2308c3a, other: 0x0, expected: Some(0x3e45798ee2308c3a) },
    Sample { operation: "sin", bits: 64, input: 0xbe45798ee2308c3a, other: 0x0, expected: Some(0xbe45798ee2308c3a) },
    Sample { operation: "sin", bits: 64, input: 0x400921fb54442d18, other: 0x0, expected: Some(0x3ca1a62633145c07) },
    Sample { operation: "sin", bits: 64, input: 0xc00921fb54442d18, other: 0x0, expected: Some(0xbca1a62633145c07) },
    Sample { operation: "sin", bits: 64, input: 0x3ff921fb54442d18, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "sin", bits: 64, input: 0x4202a05f20000000, other: 0x0, expected: Some(0xbfdf334c7896a4e3) },
    Sample { operation: "sin", bits: 64, input: 0x4415af1d78b58c40, other: 0x0, expected: Some(0xbfe4a5e605fd6450) },
    Sample { operation: "sin", bits: 64, input: 0x7ff0000000000000, other: 0x0, expected: None },
    Sample { operation: "sin", bits: 64, input: 0xfff0000000000000, other: 0x0, expected: None },
    Sample { operation: "sin", bits: 64, input: 0x7ff8000000000000, other: 0x0, expected: None },
    Sample { operation: "sin", bits: 64, input: 0x1, other: 0x0, expected: Some(0x1) },
    Sample { operation: "sin", bits: 64, input: 0x8000000000000001, other: 0x0, expected: Some(0x8000000000000001) },
    Sample { operation: "sin", bits: 64, input: 0x10000000000000, other: 0x0, expected: Some(0x10000000000000) },
    Sample { operation: "sin", bits: 64, input: 0x7fefffffffffffff, other: 0x0, expected: Some(0x3f7452fc98b34e97) },
    Sample { operation: "sin", bits: 64, input: 0xffefffffffffffff, other: 0x0, expected: Some(0xbf7452fc98b34e97) },
    Sample { operation: "sin", bits: 64, input: 0x400921fb54442d17, other: 0x0, expected: Some(0x3cc469898cc51702) },
    Sample { operation: "sin", bits: 64, input: 0x400921fb54442d19, other: 0x0, expected: Some(0xbcb72cece675d1fd) },
    Sample { operation: "sin", bits: 64, input: 0x3ff921fb54442d17, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "sin", bits: 64, input: 0x3ff921fb54442d19, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "sin", bits: 64, input: 0x7e37e43c8800759c, other: 0x0, expected: Some(0xbfea2c16b010e385) },
    Sample { operation: "cos", bits: 64, input: 0x0, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0x8000000000000000, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0x3ff0000000000000, other: 0x0, expected: Some(0x3fe14a280fb5068c) },
    Sample { operation: "cos", bits: 64, input: 0xbff0000000000000, other: 0x0, expected: Some(0x3fe14a280fb5068c) },
    Sample { operation: "cos", bits: 64, input: 0x3e45798ee2308c3a, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0xbe45798ee2308c3a, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0x400921fb54442d18, other: 0x0, expected: Some(0xbff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0xc00921fb54442d18, other: 0x0, expected: Some(0xbff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0x3ff921fb54442d18, other: 0x0, expected: Some(0x3c91a62633145c07) },
    Sample { operation: "cos", bits: 64, input: 0x4202a05f20000000, other: 0x0, expected: Some(0x3febf098901c931a) },
    Sample { operation: "cos", bits: 64, input: 0x4415af1d78b58c40, other: 0x0, expected: Some(0x3fe872720fc60d3d) },
    Sample { operation: "cos", bits: 64, input: 0x7ff0000000000000, other: 0x0, expected: None },
    Sample { operation: "cos", bits: 64, input: 0xfff0000000000000, other: 0x0, expected: None },
    Sample { operation: "cos", bits: 64, input: 0x7ff8000000000000, other: 0x0, expected: None },
    Sample { operation: "cos", bits: 64, input: 0x1, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0x8000000000000001, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0x10000000000000, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0x7fefffffffffffff, other: 0x0, expected: Some(0xbfefffe62ecfab75) },
    Sample { operation: "cos", bits: 64, input: 0xffefffffffffffff, other: 0x0, expected: Some(0xbfefffe62ecfab75) },
    Sample { operation: "cos", bits: 64, input: 0x400921fb54442d17, other: 0x0, expected: Some(0xbff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0x400921fb54442d19, other: 0x0, expected: Some(0xbff0000000000000) },
    Sample { operation: "cos", bits: 64, input: 0x3ff921fb54442d17, other: 0x0, expected: Some(0x3cb469898cc51702) },
    Sample { operation: "cos", bits: 64, input: 0x3ff921fb54442d19, other: 0x0, expected: Some(0xbca72cece675d1fd) },
    Sample { operation: "cos", bits: 64, input: 0x7e37e43c8800759c, other: 0x0, expected: Some(0xbfe2699022adc4c1) },
    Sample { operation: "atan2", bits: 64, input: 0x0, other: 0x0, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 64, input: 0x0, other: 0x8000000000000000, expected: Some(0x400921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x0, other: 0x3ff0000000000000, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 64, input: 0x0, other: 0xbff0000000000000, expected: Some(0x400921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x0, other: 0x7ff0000000000000, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 64, input: 0x0, other: 0xfff0000000000000, expected: Some(0x400921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x0, other: 0x7ff8000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x8000000000000000, other: 0x0, expected: Some(0x8000000000000000) },
    Sample { operation: "atan2", bits: 64, input: 0x8000000000000000, other: 0x8000000000000000, expected: Some(0xc00921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x8000000000000000, other: 0x3ff0000000000000, expected: Some(0x8000000000000000) },
    Sample { operation: "atan2", bits: 64, input: 0x8000000000000000, other: 0xbff0000000000000, expected: Some(0xc00921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x8000000000000000, other: 0x7ff0000000000000, expected: Some(0x8000000000000000) },
    Sample { operation: "atan2", bits: 64, input: 0x8000000000000000, other: 0xfff0000000000000, expected: Some(0xc00921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x8000000000000000, other: 0x7ff8000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x3ff0000000000000, other: 0x0, expected: Some(0x3ff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x3ff0000000000000, other: 0x8000000000000000, expected: Some(0x3ff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x3ff0000000000000, other: 0x3ff0000000000000, expected: Some(0x3fe921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x3ff0000000000000, other: 0xbff0000000000000, expected: Some(0x4002d97c7f3321d2) },
    Sample { operation: "atan2", bits: 64, input: 0x3ff0000000000000, other: 0x7ff0000000000000, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 64, input: 0x3ff0000000000000, other: 0xfff0000000000000, expected: Some(0x400921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x3ff0000000000000, other: 0x7ff8000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0xbff0000000000000, other: 0x0, expected: Some(0xbff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xbff0000000000000, other: 0x8000000000000000, expected: Some(0xbff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xbff0000000000000, other: 0x3ff0000000000000, expected: Some(0xbfe921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xbff0000000000000, other: 0xbff0000000000000, expected: Some(0xc002d97c7f3321d2) },
    Sample { operation: "atan2", bits: 64, input: 0xbff0000000000000, other: 0x7ff0000000000000, expected: Some(0x8000000000000000) },
    Sample { operation: "atan2", bits: 64, input: 0xbff0000000000000, other: 0xfff0000000000000, expected: Some(0xc00921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xbff0000000000000, other: 0x7ff8000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x7ff0000000000000, other: 0x0, expected: Some(0x3ff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x7ff0000000000000, other: 0x8000000000000000, expected: Some(0x3ff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x7ff0000000000000, other: 0x3ff0000000000000, expected: Some(0x3ff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x7ff0000000000000, other: 0xbff0000000000000, expected: Some(0x3ff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x7ff0000000000000, other: 0x7ff0000000000000, expected: Some(0x3fe921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x7ff0000000000000, other: 0xfff0000000000000, expected: Some(0x4002d97c7f3321d2) },
    Sample { operation: "atan2", bits: 64, input: 0x7ff0000000000000, other: 0x7ff8000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0xfff0000000000000, other: 0x0, expected: Some(0xbff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xfff0000000000000, other: 0x8000000000000000, expected: Some(0xbff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xfff0000000000000, other: 0x3ff0000000000000, expected: Some(0xbff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xfff0000000000000, other: 0xbff0000000000000, expected: Some(0xbff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xfff0000000000000, other: 0x7ff0000000000000, expected: Some(0xbfe921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xfff0000000000000, other: 0xfff0000000000000, expected: Some(0xc002d97c7f3321d2) },
    Sample { operation: "atan2", bits: 64, input: 0xfff0000000000000, other: 0x7ff8000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x7ff8000000000000, other: 0x0, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x7ff8000000000000, other: 0x8000000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x7ff8000000000000, other: 0x3ff0000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x7ff8000000000000, other: 0xbff0000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x7ff8000000000000, other: 0x7ff0000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x7ff8000000000000, other: 0xfff0000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x7ff8000000000000, other: 0x7ff8000000000000, expected: None },
    Sample { operation: "atan2", bits: 64, input: 0x1, other: 0x7fefffffffffffff, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 64, input: 0x7fefffffffffffff, other: 0x1, expected: Some(0x3ff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x10000000000000, other: 0x7fefffffffffffff, expected: Some(0x0) },
    Sample { operation: "atan2", bits: 64, input: 0x7fefffffffffffff, other: 0x10000000000000, expected: Some(0x3ff921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x3ff0000000000000, other: 0xc000000000000000, expected: Some(0x40056c6e7397f5ae) },
    Sample { operation: "atan2", bits: 64, input: 0xbff0000000000000, other: 0x4000000000000000, expected: Some(0xbfddac670561bb4f) },
    Sample { operation: "atan2", bits: 32, input: 0x3c8ef858, other: 0x3f7ff605, expected: Some(0x3c8efa34) },
    Sample { operation: "atan2", bits: 32, input: 0x3c8ef859, other: 0x3f7ff605, expected: Some(0x3c8efa35) },
    Sample { operation: "atan2", bits: 32, input: 0x3c8ef85a, other: 0x3f7ff605, expected: Some(0x3c8efa36) },
    Sample { operation: "atan2", bits: 32, input: 0x3f3504f2, other: 0x3f3504f3, expected: Some(0x3f490fda) },
    Sample { operation: "atan2", bits: 32, input: 0x3f3504f3, other: 0x3f3504f3, expected: Some(0x3f490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x3f3504f4, other: 0x3f3504f3, expected: Some(0x3f490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x3f7ff604, other: 0x3c8ef859, expected: Some(0x3fc6d3f2) },
    Sample { operation: "atan2", bits: 32, input: 0x3f7ff605, other: 0x3c8ef859, expected: Some(0x3fc6d3f2) },
    Sample { operation: "atan2", bits: 32, input: 0x3f7ff606, other: 0x3c8ef859, expected: Some(0x3fc6d3f2) },
    Sample { operation: "atan2", bits: 32, input: 0x3f3504f2, other: 0xbf3504f3, expected: Some(0x4016cbe4) },
    Sample { operation: "atan2", bits: 32, input: 0x3f3504f3, other: 0xbf3504f3, expected: Some(0x4016cbe4) },
    Sample { operation: "atan2", bits: 32, input: 0x3f3504f4, other: 0xbf3504f3, expected: Some(0x4016cbe4) },
    Sample { operation: "atan2", bits: 32, input: 0xbc8ef858, other: 0x3f7ff605, expected: Some(0xbc8efa34) },
    Sample { operation: "atan2", bits: 32, input: 0xbc8ef859, other: 0x3f7ff605, expected: Some(0xbc8efa35) },
    Sample { operation: "atan2", bits: 32, input: 0xbc8ef85a, other: 0x3f7ff605, expected: Some(0xbc8efa36) },
    Sample { operation: "atan2", bits: 32, input: 0xbf3504f2, other: 0x3f3504f3, expected: Some(0xbf490fda) },
    Sample { operation: "atan2", bits: 32, input: 0xbf3504f3, other: 0x3f3504f3, expected: Some(0xbf490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0xbf3504f4, other: 0x3f3504f3, expected: Some(0xbf490fdb) },
    Sample { operation: "atan2", bits: 32, input: 0x3c8ef858, other: 0xbf7ff605, expected: Some(0x4047f1e6) },
    Sample { operation: "atan2", bits: 32, input: 0x3c8ef859, other: 0xbf7ff605, expected: Some(0x4047f1e6) },
    Sample { operation: "atan2", bits: 32, input: 0x3c8ef85a, other: 0xbf7ff605, expected: Some(0x4047f1e6) },
    Sample { operation: "atan2", bits: 64, input: 0x3f91df0b2b89dd1d, other: 0x3feffec097f5af8a, expected: Some(0x3f91df46a2529d38) },
    Sample { operation: "atan2", bits: 64, input: 0x3f91df0b2b89dd1e, other: 0x3feffec097f5af8a, expected: Some(0x3f91df46a2529d39) },
    Sample { operation: "atan2", bits: 64, input: 0x3f91df0b2b89dd1f, other: 0x3feffec097f5af8a, expected: Some(0x3f91df46a2529d3a) },
    Sample { operation: "atan2", bits: 64, input: 0x3fe6a09e667f3bcb, other: 0x3fe6a09e667f3bcd, expected: Some(0x3fe921fb54442d17) },
    Sample { operation: "atan2", bits: 64, input: 0x3fe6a09e667f3bcc, other: 0x3fe6a09e667f3bcd, expected: Some(0x3fe921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x3fe6a09e667f3bcd, other: 0x3fe6a09e667f3bcd, expected: Some(0x3fe921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x3feffec097f5af89, other: 0x3f91df0b2b89dcf7, expected: Some(0x3ff8da7e39bae2a4) },
    Sample { operation: "atan2", bits: 64, input: 0x3feffec097f5af8a, other: 0x3f91df0b2b89dcf7, expected: Some(0x3ff8da7e39bae2a4) },
    Sample { operation: "atan2", bits: 64, input: 0x3feffec097f5af8b, other: 0x3f91df0b2b89dcf7, expected: Some(0x3ff8da7e39bae2a4) },
    Sample { operation: "atan2", bits: 64, input: 0x3fe6a09e667f3bcc, other: 0xbfe6a09e667f3bcc, expected: Some(0x4002d97c7f3321d2) },
    Sample { operation: "atan2", bits: 64, input: 0x3fe6a09e667f3bcd, other: 0xbfe6a09e667f3bcc, expected: Some(0x4002d97c7f3321d2) },
    Sample { operation: "atan2", bits: 64, input: 0x3fe6a09e667f3bce, other: 0xbfe6a09e667f3bcc, expected: Some(0x4002d97c7f3321d2) },
    Sample { operation: "atan2", bits: 64, input: 0xbf91df0b2b89dd1d, other: 0x3feffec097f5af8a, expected: Some(0xbf91df46a2529d38) },
    Sample { operation: "atan2", bits: 64, input: 0xbf91df0b2b89dd1e, other: 0x3feffec097f5af8a, expected: Some(0xbf91df46a2529d39) },
    Sample { operation: "atan2", bits: 64, input: 0xbf91df0b2b89dd1f, other: 0x3feffec097f5af8a, expected: Some(0xbf91df46a2529d3a) },
    Sample { operation: "atan2", bits: 64, input: 0xbfe6a09e667f3bcb, other: 0x3fe6a09e667f3bcd, expected: Some(0xbfe921fb54442d17) },
    Sample { operation: "atan2", bits: 64, input: 0xbfe6a09e667f3bcc, other: 0x3fe6a09e667f3bcd, expected: Some(0xbfe921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0xbfe6a09e667f3bcd, other: 0x3fe6a09e667f3bcd, expected: Some(0xbfe921fb54442d18) },
    Sample { operation: "atan2", bits: 64, input: 0x3f91df0b2b89dd08, other: 0xbfeffec097f5af8a, expected: Some(0x4008fe3cc6ff87de) },
    Sample { operation: "atan2", bits: 64, input: 0x3f91df0b2b89dd09, other: 0xbfeffec097f5af8a, expected: Some(0x4008fe3cc6ff87de) },
    Sample { operation: "atan2", bits: 64, input: 0x3f91df0b2b89dd0a, other: 0xbfeffec097f5af8a, expected: Some(0x4008fe3cc6ff87de) },
    Sample { operation: "pair", bits: 32, input: 0x0, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "pair", bits: 32, input: 0x80000000, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "pair", bits: 32, input: 0x3f800000, other: 0x0, expected: Some(0x3fb0ddf2) },
    Sample { operation: "pair", bits: 32, input: 0xbf800000, other: 0x0, expected: Some(0xbe9a32c8) },
    Sample { operation: "pair", bits: 32, input: 0x322bcc77, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "pair", bits: 32, input: 0xb22bcc77, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "pair", bits: 32, input: 0x40490fdb, other: 0x0, expected: Some(0xbf800001) },
    Sample { operation: "pair", bits: 32, input: 0xc0490fdb, other: 0x0, expected: Some(0xbf7fffff) },
    Sample { operation: "pair", bits: 32, input: 0x3fc90fdb, other: 0x0, expected: Some(0x3f7fffff) },
    Sample { operation: "pair", bits: 32, input: 0x501502f9, other: 0x0, expected: Some(0x3ec56f26) },
    Sample { operation: "pair", bits: 32, input: 0x60ad78ec, other: 0x0, expected: Some(0x3fb49646) },
    Sample { operation: "pair", bits: 32, input: 0x7f800000, other: 0x0, expected: None },
    Sample { operation: "pair", bits: 32, input: 0xff800000, other: 0x0, expected: None },
    Sample { operation: "pair", bits: 32, input: 0x7fc00000, other: 0x0, expected: None },
    Sample { operation: "pair", bits: 32, input: 0x1, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "pair", bits: 32, input: 0x80000001, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "pair", bits: 32, input: 0x800000, other: 0x0, expected: Some(0x3f800000) },
    Sample { operation: "pair", bits: 32, input: 0x7f7fffff, other: 0x0, expected: Some(0x3ea98bc6) },
    Sample { operation: "pair", bits: 32, input: 0xff7fffff, other: 0x0, expected: Some(0x3faffca4) },
    Sample { operation: "pair", bits: 32, input: 0x40490fda, other: 0x0, expected: Some(0xbf7ffffd) },
    Sample { operation: "pair", bits: 32, input: 0x40490fdc, other: 0x0, expected: Some(0xbf800003) },
    Sample { operation: "pair", bits: 32, input: 0x3fc90fda, other: 0x0, expected: Some(0x3f800001) },
    Sample { operation: "pair", bits: 32, input: 0x3fc90fdc, other: 0x0, expected: Some(0x3f7ffffd) },
    Sample { operation: "pair", bits: 64, input: 0x0, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "pair", bits: 64, input: 0x8000000000000000, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "pair", bits: 64, input: 0x3ff0000000000000, other: 0x0, expected: Some(0x3ff61bbe4f5f09bd) },
    Sample { operation: "pair", bits: 64, input: 0xbff0000000000000, other: 0x0, expected: Some(0xbfd34658fea80cc4) },
    Sample { operation: "pair", bits: 64, input: 0x3e45798ee2308c3a, other: 0x0, expected: Some(0x3ff0000002af31dc) },
    Sample { operation: "pair", bits: 64, input: 0xbe45798ee2308c3a, other: 0x0, expected: Some(0x3feffffffaa19c47) },
    Sample { operation: "pair", bits: 64, input: 0x400921fb54442d18, other: 0x0, expected: Some(0xbfefffffffffffff) },
    Sample { operation: "pair", bits: 64, input: 0xc00921fb54442d18, other: 0x0, expected: Some(0xbff0000000000001) },
    Sample { operation: "pair", bits: 64, input: 0x3ff921fb54442d18, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "pair", bits: 64, input: 0x4202a05f20000000, other: 0x0, expected: Some(0x3fd8ade4a7a28151) },
    Sample { operation: "pair", bits: 64, input: 0x4415af1d78b58c40, other: 0x0, expected: Some(0x3fbe64604e454768) },
    Sample { operation: "pair", bits: 64, input: 0x7ff0000000000000, other: 0x0, expected: None },
    Sample { operation: "pair", bits: 64, input: 0xfff0000000000000, other: 0x0, expected: None },
    Sample { operation: "pair", bits: 64, input: 0x7ff8000000000000, other: 0x0, expected: None },
    Sample { operation: "pair", bits: 64, input: 0x1, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "pair", bits: 64, input: 0x8000000000000001, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "pair", bits: 64, input: 0x10000000000000, other: 0x0, expected: Some(0x3ff0000000000000) },
    Sample { operation: "pair", bits: 64, input: 0x7fefffffffffffff, other: 0x0, expected: Some(0xbfefd740359e44d8) },
    Sample { operation: "pair", bits: 64, input: 0xffefffffffffffff, other: 0x0, expected: Some(0xbff0144614008909) },
    Sample { operation: "pair", bits: 64, input: 0x400921fb54442d17, other: 0x0, expected: Some(0xbfeffffffffffffb) },
    Sample { operation: "pair", bits: 64, input: 0x400921fb54442d19, other: 0x0, expected: Some(0xbff0000000000001) },
    Sample { operation: "pair", bits: 64, input: 0x3ff921fb54442d17, other: 0x0, expected: Some(0x3ff0000000000001) },
    Sample { operation: "pair", bits: 64, input: 0x3ff921fb54442d19, other: 0x0, expected: Some(0x3fefffffffffffff) },
    Sample { operation: "pair", bits: 64, input: 0x7e37e43c8800759c, other: 0x0, expected: Some(0xbff64ad3695f5423) },
];

/// C supplies exact IEEE inputs and checks independent expected bits. It never computes math.
pub fn sources() -> (String, String, usize) {
    let mut c = String::from(
        "#include <stdint.h>\n#include <string.h>\n#include <math.h>\n#include <stdio.h>\n#include <stdlib.h>\nstatic int64_t observed;\nint64_t trig_observed(void){return observed;}\n",
    );
    let mut source =
        String::from("extern \"C\" fn trig_observed() -> i64\npub fn keep<T>(value:T)->T=value\n");
    let mut main = String::from("pub fn exercise() -> i32 { unsafe {\n");
    let mut count = 0;
    for bits in [32, 64] {
        let ty = if bits == 32 { "float" } else { "double" };
        let int = format!("uint{bits}_t");
        let sign = 1u64 << (bits - 1);
        let exponent = if bits == 32 {
            0x7f80_0000u64
        } else {
            0x7ff0_0000_0000_0000
        };
        let rows = SAMPLES
            .iter()
            .filter(|row| row.bits == bits)
            .collect::<Vec<_>>();
        c.push_str(&format!(
            "static const struct {{ {int} input,other,expected; int nan,exact,unit_range; }} samples{bits}[]={{\n"
        ));
        for row in &rows {
            c.push_str(&format!(
                "{{{}ULL,{}ULL,{}ULL,{},{},{}}},\n",
                row.input,
                row.other,
                row.expected.unwrap_or(0),
                i32::from(row.expected.is_none()),
                i32::from(row.operation == "cos" && row.input & !sign == 0),
                i32::from(matches!(row.operation, "sin" | "cos"))
            ));
        }
        c.push_str(&format!("}};\n{ty} trig_get{bits}(int64_t i,int64_t operand){{if(operand<0||operand>1||i<0||i>={})abort(); {ty} x; memcpy(&x,operand?&samples{bits}[i].other:&samples{bits}[i].input,sizeof(x));return x;}}\n",rows.len()));
        c.push_str(&format!("int32_t trig_check{bits}({ty} x,int64_t i){{if(i<0||i>={})abort();observed++; if(samples{bits}[i].nan)return isnan(x)?0:1; if(isnan(x))return 3; if(samples{bits}[i].unit_range && (x < -1 || x > 1))return 4; {int} actual;memcpy(&actual,&x,sizeof(x)); {int} expected=samples{bits}[i].expected; {int} a=(actual&{sign}ULL)?({int})~actual:(actual|{sign}ULL); {int} e=(expected&{sign}ULL)?({int})~expected:(expected|{sign}ULL); {int} distance=a>e?a-e:e-a; int exact=samples{bits}[i].exact||(expected&~{sign}ULL)==0||(expected&{exponent}ULL)=={exponent}ULL; if(distance>(exact?0:4)){{fprintf(stderr,\"trig{bits} sample %lld differs by %llu ULP\\n\",(long long)i,(unsigned long long)distance);return 2;}}return 0;}}\n",rows.len()));
        source.push_str(&format!("extern \"C\" fn trig_get{bits}(index:i64,operand:i64)->f{bits}\nextern \"C\" fn trig_check{bits}(value:f{bits},index:i64)->i32\n"));
        for operation in ["sin", "cos", "atan2", "pair"] {
            let indices = rows
                .iter()
                .enumerate()
                .filter_map(|(i, row)| (row.operation == operation).then_some(i))
                .collect::<Vec<_>>();
            for lanes in [1, 2, 4, 8, 16] {
                let name = format!("case_{operation}_{bits}_{lanes}");
                let value_ty = if lanes == 1 {
                    format!("f{bits}")
                } else {
                    format!("vec{lanes}<f{bits}>")
                };
                let other_parameter = if operation == "atan2" {
                    format!(",other:{value_ty}")
                } else {
                    String::new()
                };
                let expression = match operation {
                    "atan2" => "value.atan2(other)".to_owned(),
                    "pair" => "value.sin()+value.cos()".to_owned(),
                    _ => format!("value.{operation}()"),
                };
                source.push_str(&format!(
                    "pub fn {name}(value:{value_ty}{other_parameter}) -> {value_ty} = keep({expression})\n"
                ));
                let runner = format!("run_{operation}_{bits}_{lanes}");
                let mut body = format!("fn {runner}()->i32 {{ unsafe {{\n");
                for (batch, chunk) in indices.chunks(lanes).enumerate() {
                    let selected = (0..lanes)
                        .map(|i| chunk[i % chunk.len()])
                        .collect::<Vec<_>>();
                    let inputs = selected
                        .iter()
                        .map(|i| format!("trig_get{bits}({i},0)"))
                        .collect::<Vec<_>>();
                    let input = if lanes == 1 {
                        inputs[0].clone()
                    } else {
                        format!("[{}]", inputs.join(","))
                    };
                    let argument = if operation == "atan2" {
                        let inputs = selected
                            .iter()
                            .map(|i| format!("trig_get{bits}({i},1)"))
                            .collect::<Vec<_>>();
                        let value = if lanes == 1 {
                            inputs[0].clone()
                        } else {
                            format!("[{}]", inputs.join(","))
                        };
                        format!(",{value}")
                    } else {
                        String::new()
                    };
                    body.push_str(&format!("value{batch}:={name}({input}{argument})\n"));
                    for (lane, index) in selected.iter().enumerate() {
                        let value = if lanes == 1 {
                            format!("value{batch}")
                        } else {
                            format!("value{batch}[{lane}]")
                        };
                        body.push_str(&format!(
                            "if trig_check{bits}({value},{index})!=0 {{ return 1 }}\n"
                        ));
                        count += 1;
                    }
                }
                body.push_str("return 0\n} }\n");
                source.push_str(&body);
                main.push_str(&format!("if {runner}()!=0 {{ return 1 }}\n"));
            }
        }
    }
    main.push_str(&format!(
        "if trig_observed()!={count} {{ return 2 }}\nreturn 0\n}} }}\n"
    ));
    source.push_str(&main);
    (c, source, count)
}
