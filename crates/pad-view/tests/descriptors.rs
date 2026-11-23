//! The parser against real, published descriptors (see tests/data).

mod data;

use pad_view::decode::{Hints, Layout, choose};
use pad_view::descriptor::{Descriptor, Kind};

fn bytes_of(d: &Descriptor, kind: Kind, id: u8) -> Option<u32> {
    d.reports()
        .iter()
        .find(|r| r.kind == kind && r.id == id)
        .map(|r| r.bytes(d.uses_report_ids))
}

fn no_hints() -> Hints<'static> {
    Hints {
        name: "",
        vendor_id: None,
        product_id: None,
    }
}

#[test]
fn switch_pro() {
    let d = Descriptor::parse(&data::SWITCH_PRO);
    assert!(!d.damaged);
    assert_eq!(d.dropped, 0);
    assert!(d.uses_report_ids);
    // Sizes with the report id byte, as they arrive.
    assert_eq!(bytes_of(&d, Kind::Input, 0x3F), Some(12));
    assert_eq!(bytes_of(&d, Kind::Input, 0x30), Some(49));
    assert_eq!(bytes_of(&d, Kind::Input, 0x21), Some(49));
    assert_eq!(bytes_of(&d, Kind::Output, 0x01), Some(49));
    assert_eq!(d.summary(), (16, 4, 1));
    assert_eq!(choose(&no_hints(), &d, None).0, Layout::SwitchPro);
}

#[test]
fn gulikit_android_mode_is_the_xbox_one_s_descriptor() {
    let d = Descriptor::parse(&data::GULIKIT_KK2_ANDROID);
    assert!(!d.damaged);
    assert_eq!(d.dropped, 0);
    assert_eq!(bytes_of(&d, Kind::Input, 0x01), Some(16));
    assert_eq!(bytes_of(&d, Kind::Input, 0x02), Some(2));
    assert_eq!(bytes_of(&d, Kind::Input, 0x04), Some(2));
    assert!(bytes_of(&d, Kind::Output, 0x03).is_some());
    // 10 buttons, 6 axes (2 sticks, 2 triggers), 1 hat.
    assert_eq!(d.summary(), (10, 6, 1));
    // Even with no name and no ids, its shape says Xbox.
    assert_eq!(choose(&no_hints(), &d, None).0, Layout::Xbox);
    // The Guide button lives alone in report 2 (System Main Menu).
    assert!(d.has_input_usage(0x01, 0x85));
}

#[test]
fn xbox_one_s_newer_firmware() {
    let d = Descriptor::parse(&data::XBOX_ONE_S_02FD);
    assert!(!d.damaged);
    assert_eq!(bytes_of(&d, Kind::Input, 0x01), Some(17));
    assert_eq!(choose(&no_hints(), &d, None).0, Layout::Xbox);
    let h = Hints {
        name: "Xbox Wireless Controller",
        vendor_id: Some(0x045E),
        product_id: Some(0x02FD),
    };
    assert_eq!(choose(&h, &d, None).0, Layout::Xbox);
}

#[test]
fn dualshock4() {
    let d = Descriptor::parse(&data::DUALSHOCK4_BT);
    assert!(!d.damaged);
    assert_eq!(d.dropped, 0, "all of its many reports fit");
    assert_eq!(bytes_of(&d, Kind::Input, 0x01), Some(10));
    assert_eq!(bytes_of(&d, Kind::Input, 0x11), Some(78));
    let h = Hints {
        name: "Wireless Controller",
        vendor_id: Some(0x054C),
        product_id: Some(0x05C4),
    };
    assert_eq!(choose(&h, &d, None).0, Layout::PlayStation);
    assert_eq!(choose(&no_hints(), &d, None).0, Layout::Generic);
}

#[test]
fn generic_android_pads() {
    for bytes in [&data::ASUS_ANDROID[..], &data::RAZER_SERVAL[..]] {
        let d = Descriptor::parse(bytes);
        assert!(!d.damaged);
        assert_eq!(choose(&no_hints(), &d, None).0, Layout::Android);
        // X, Y, Z, Rz on the Generic Desktop page, Brake and Gas on the
        // Simulation page, in one Input item.
        assert!(d.has_input_usage(0x01, 0x30));
        assert!(d.has_input_usage(0x01, 0x35));
        assert!(d.has_input_usage(0x02, 0xC5));
        assert!(d.has_input_usage(0x02, 0xC4));
        // Back and Home are Consumer usages next to the buttons.
        assert!(d.has_input_usage(0x0C, 0x224));
        assert!(d.has_input_usage(0x0C, 0x223));
    }
    // The Asus pad declares its buttons with 2-byte usages and then
    // switches page for Back and Home: the buttons must stay buttons.
    let d = Descriptor::parse(&data::ASUS_ANDROID);
    assert_eq!(d.summary().0, 9);
    assert_eq!(bytes_of(&d, Kind::Input, 0x01), Some(9));
}

#[test]
fn usage_page_after_the_usages() {
    // Usage(X) Usage(Y) declared while the page is 0, then the Usage Page
    // item, then Input: the page applies to X and Y.
    let desc = [
        0x09, 0x30, 0x09, 0x31, 0x05, 0x01, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x02,
        0x81, 0x02,
    ];
    let d = Descriptor::parse(&desc);
    assert!(d.has_input_usage(0x01, 0x30));
    assert!(d.has_input_usage(0x01, 0x31));
}

#[test]
fn logical_max_ff_in_one_byte_is_255() {
    // Logical Maximum 0xFF in one byte would be -1 if read as signed.
    let desc = [
        0x05, 0x01, 0x09, 0x30, 0x15, 0x00, 0x25, 0xFF, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02,
    ];
    let d = Descriptor::parse(&desc);
    assert_eq!(d.fields()[0].logical_max, 255);
    assert_eq!(d.fields()[0].value(&[200], 0), Some(200));
}
