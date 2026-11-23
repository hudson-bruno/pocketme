//! The HCI rules on the PC: lengths, the gate, the commands, the events,
//! the start-up sequence, and that addresses never print in full.

use hci_rules::{
    BdAddr, Cmd, EVENT_MASK, Event, Gate, Refusal, Setup, Step, answer_connection_request, gate,
    op, packet_len, parse_event,
};

/// A made-up address (written AA:BB:CC:DD:EE:F1), in HCI order.
const ADDR: [u8; 6] = [0xF1, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA];

fn event(code: u8, params: &[u8]) -> Vec<u8> {
    let mut p = vec![0x04, code, params.len() as u8];
    p.extend_from_slice(params);
    p
}

fn command(opcode: u16, params: &[u8]) -> Vec<u8> {
    let [lo, hi] = opcode.to_le_bytes();
    let mut p = vec![0x01, lo, hi, params.len() as u8];
    p.extend_from_slice(params);
    p
}

fn complete(opcode: u16, credits: u8, ret: &[u8]) -> Vec<u8> {
    let [lo, hi] = opcode.to_le_bytes();
    let mut params = vec![credits, lo, hi];
    params.extend_from_slice(ret);
    event(0x0E, &params)
}

// ---- lengths ----------------------------------------------------------

#[test]
fn lengths_come_from_the_header() {
    assert_eq!(packet_len(&[0x04, 0x0E, 4]), Some(7));
    assert_eq!(packet_len(&[0x04, 0x0E, 255]), Some(258));
    assert_eq!(packet_len(&[0x01, 0x03, 0x0C, 0]), Some(4));
    assert_eq!(packet_len(&[0x02, 0x01, 0x20, 0xFD, 0x03]), Some(5 + 1021));
    assert_eq!(packet_len(&[0x03, 0x01, 0x00, 60]), Some(64));
}

#[test]
fn lengths_need_the_whole_header_and_a_known_kind() {
    assert_eq!(packet_len(&[]), None);
    assert_eq!(packet_len(&[0x04, 0x0E]), None);
    assert_eq!(packet_len(&[0x02, 0x01, 0x20, 0xFD]), None);
    assert_eq!(packet_len(&[0x01, 0x03, 0x0C]), None);
    assert_eq!(packet_len(&[0x05, 0, 0, 0, 0]), None);
    assert_eq!(packet_len(&[0x00, 0x0E, 4]), None);
}

#[test]
fn the_largest_packet_fits_the_buffer() {
    const { assert!(5 + 1021 <= hci_rules::MAX_PACKET) };
    const { assert!(3 + 255 <= hci_rules::MAX_PACKET) };
}

// ---- commands ---------------------------------------------------------

#[test]
fn commands_have_the_right_bytes() {
    assert_eq!(Cmd::reset().as_bytes(), &[0x01, 0x03, 0x0C, 0]);
    assert_eq!(Cmd::read_local_version().as_bytes(), &[0x01, 0x01, 0x10, 0]);
    assert_eq!(Cmd::read_buffer_size().as_bytes(), &[0x01, 0x05, 0x10, 0]);
    assert_eq!(
        Cmd::write_scan_enable(true).as_bytes(),
        &[0x01, 0x1A, 0x0C, 1, 0x02]
    );
    assert_eq!(
        Cmd::write_scan_enable(false).as_bytes(),
        &[0x01, 0x1A, 0x0C, 1, 0x00]
    );
    let m = Cmd::set_event_mask(0x0102_0304_0506_0708);
    assert_eq!(m.as_bytes(), &[0x01, 0x01, 0x0C, 8, 8, 7, 6, 5, 4, 3, 2, 1]);
    let r = Cmd::reject_connection(&BdAddr::new(ADDR));
    assert_eq!(
        r.as_bytes(),
        &[
            0x01, 0x0A, 0x04, 7, 0xF1, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA, 0x0F
        ]
    );
    assert_eq!(r.opcode(), op::REJECT_CONNECTION_REQUEST);
    // Its Debug form leaves the address out.
    assert_eq!(format!("{r:?}"), "Cmd(0x040a)");
}

#[test]
fn the_event_mask_leaves_out_search_results_only() {
    assert_eq!(EVENT_MASK & 0b11, 0, "inquiry complete / result");
    assert_eq!(EVENT_MASK & (1 << 33), 0, "inquiry result with RSSI");
    // Connection request (bit 3), connection complete (2), disconnection
    // (4), link key request (22), hardware error (15) stay on.
    for bit in [2, 3, 4, 15, 22] {
        assert_ne!(EVENT_MASK & (1 << bit), 0, "bit {bit}");
    }
    assert_eq!(EVENT_MASK >> 45, 0, "nothing past the default set");
}

#[test]
fn every_command_built_here_passes_the_gate() {
    let all = [
        Cmd::reset(),
        Cmd::read_local_version(),
        Cmd::read_buffer_size(),
        Cmd::set_event_mask(EVENT_MASK),
        Cmd::write_scan_enable(true),
        Cmd::write_scan_enable(false),
        Cmd::reject_connection(&BdAddr::new(ADDR)),
    ];
    for c in all {
        assert_eq!(gate(c.as_bytes()), Ok(()), "{:#06x}", c.opcode());
    }
}

// ---- the gate -----------------------------------------------------------

/// Another made-up address: a device that is not a saved pad.
const OTHER: [u8; 6] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];

fn gate_for(saved: &[[u8; 6]]) -> Gate {
    let mut g = Gate::new();
    for a in saved {
        assert!(g.allow(*a));
    }
    g
}

#[test]
fn the_gate_refuses_searching_calling_and_saying_yes_to_pairing() {
    let g = gate_for(&[ADDR]);
    let refused = [
        (op::INQUIRY, &[0x33, 0x8B, 0x9E, 8, 0][..]),
        (0x0402, &[][..]),                     // Inquiry Cancel
        (op::CREATE_CONNECTION, &[0; 13][..]), // even to a saved pad
        (0x0408, &[0; 6][..]),                 // Create Connection Cancel
        (op::PIN_CODE_REQUEST_REPLY, &[0; 23][..]),
        (op::IO_CAPABILITY_REQUEST_REPLY, &[0; 9][..]),
        (op::USER_CONFIRMATION_REQUEST_REPLY, &[0; 6][..]),
        (0x0C05, &[0; 1][..]),  // Set Event Filter
        (0x0C11, &[0; 23][..]), // Write Stored Link Key
        (0x0C12, &[0; 7][..]),  // Delete Stored Link Key
        (0x0C20, &[1][..]),     // Write Authentication Enable
        (0x200C, &[1, 0][..]),  // LE Set Scan Enable
        (0x200A, &[1][..]),     // LE Set Advertising Enable
        (0x200D, &[0; 25][..]), // LE Create Connection
    ];
    for (opcode, params) in refused {
        let mut c = command(opcode, params);
        if opcode == op::CREATE_CONNECTION {
            c[4..10].copy_from_slice(&ADDR);
        }
        assert_eq!(
            g.check(&c),
            Err(Refusal::NotAllowed(opcode)),
            "{opcode:#06x}"
        );
    }
}

#[test]
fn the_gate_never_lets_the_console_be_discoverable() {
    for v in [0x01, 0x03, 0x04, 0xFF] {
        assert_eq!(
            gate(&command(op::WRITE_SCAN_ENABLE, &[v])),
            Err(Refusal::BadParams(op::WRITE_SCAN_ENABLE)),
            "{v:#04x}"
        );
    }
    assert_eq!(gate(&command(op::WRITE_SCAN_ENABLE, &[0x02])), Ok(()));
    assert_eq!(gate(&command(op::WRITE_SCAN_ENABLE, &[0x00])), Ok(()));
    assert_eq!(
        gate(&command(op::WRITE_SCAN_ENABLE, &[0x02, 0x00])),
        Err(Refusal::BadParams(op::WRITE_SCAN_ENABLE))
    );
}

/// An accept, a key or a name request goes only to a saved pad: never
/// to anyone else, and never when nothing is saved.
#[test]
fn the_gate_accepts_and_gives_keys_to_saved_pads_only() {
    let accept = |a: [u8; 6], role: u8| {
        let mut p = a.to_vec();
        p.push(role);
        command(op::ACCEPT_CONNECTION_REQUEST, &p)
    };
    let key = |a: [u8; 6]| {
        let mut p = a.to_vec();
        p.extend_from_slice(&[0x5A; 16]);
        command(op::LINK_KEY_REQUEST_REPLY, &p)
    };
    let name = |a: [u8; 6]| {
        let mut p = a.to_vec();
        p.extend_from_slice(&[1, 0, 0, 0]);
        command(op::REMOTE_NAME_REQUEST, &p)
    };
    let none = Gate::new();
    let g = gate_for(&[ADDR]);
    for (c, opc) in [
        (accept(ADDR, 1), op::ACCEPT_CONNECTION_REQUEST),
        (key(ADDR), op::LINK_KEY_REQUEST_REPLY),
        (name(ADDR), op::REMOTE_NAME_REQUEST),
    ] {
        assert_eq!(none.check(&c), Err(Refusal::NotSaved(opc)), "{opc:#06x}");
        assert_eq!(g.check(&c), Ok(()), "{opc:#06x}");
    }
    assert_eq!(
        gate(&accept(ADDR, 1)),
        Err(Refusal::NotSaved(op::ACCEPT_CONNECTION_REQUEST))
    );
    assert_eq!(g.check(&accept(ADDR, 0)), Ok(()), "become central: allowed");
    for c in [accept(OTHER, 1), key(OTHER), name(OTHER)] {
        assert!(matches!(g.check(&c), Err(Refusal::NotSaved(_))));
    }
    assert_eq!(
        g.check(&accept(ADDR, 2)),
        Err(Refusal::BadParams(op::ACCEPT_CONNECTION_REQUEST))
    );
    // Saying no is always allowed, to anyone.
    assert_eq!(
        none.check(&command(op::REJECT_CONNECTION_REQUEST, &[0; 7])),
        Ok(())
    );
    assert_eq!(
        none.check(&command(op::LINK_KEY_REQUEST_NEGATIVE_REPLY, &OTHER)),
        Ok(())
    );
    // At most four saved pads; the same one twice takes one place.
    let mut full = Gate::new();
    for i in 0..4 {
        assert!(full.allow([i, 0, 0, 0, 0, 0]));
    }
    assert!(full.allow([0, 0, 0, 0, 0, 0]));
    assert!(!full.allow(OTHER));
    assert_eq!(full.saved(), 4);
    assert_eq!(format!("{full:?}"), "Gate(4 saved, 0 linked)");
}

/// Everything the reconnect-only host sends, with its own lengths.
#[test]
fn the_gate_lets_the_reconnect_only_host_through() {
    let g = gate_for(&[ADDR]);
    let mut name = [0u8; 248];
    name[..8].copy_from_slice(b"PocketMe");
    let allowed: [(u16, &[u8]); 26] = [
        (op::RESET, &[]),
        (op::READ_LOCAL_VERSION, &[]),
        (op::READ_LOCAL_FEATURES, &[]),
        (op::READ_BUFFER_SIZE, &[]),
        (op::READ_BD_ADDR, &[]),
        (op::SET_EVENT_MASK, &[0xFF; 8]),
        (op::WRITE_SIMPLE_PAIRING_MODE, &[1]),
        (op::WRITE_DEFAULT_LINK_POLICY_SETTINGS, &[5, 0]),
        (op::WRITE_LOCAL_NAME, &name),
        (op::WRITE_CLASS_OF_DEVICE, &[0x0C, 0x01, 0x00]),
        (op::WRITE_INQUIRY_MODE, &[2]),
        (op::WRITE_PAGE_TIMEOUT, &[0, 0x20]),
        (op::WRITE_SCAN_ENABLE, &[2]),
        (op::REJECT_CONNECTION_REQUEST, &[0; 7]),
        (op::LINK_KEY_REQUEST_NEGATIVE_REPLY, &[0; 6]),
        (op::PIN_CODE_REQUEST_NEGATIVE_REPLY, &[0; 6]),
        (op::IO_CAPABILITY_REQUEST_NEGATIVE_REPLY, &[0; 7]),
        (op::USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY, &[0; 6]),
        (op::USER_PASSKEY_REQUEST_NEGATIVE_REPLY, &[0; 6]),
        (op::AUTHENTICATION_REQUESTED, &[0x80, 0]),
        (op::SET_CONNECTION_ENCRYPTION, &[0x80, 0, 1]),
        (op::READ_REMOTE_SUPPORTED_FEATURES, &[0x80, 0]),
        (op::READ_REMOTE_EXTENDED_FEATURES, &[0x80, 0, 1]),
        (op::READ_REMOTE_VERSION_INFORMATION, &[0x80, 0]),
        (op::READ_ENCRYPTION_KEY_SIZE, &[0x80, 0]),
        (op::DISCONNECT, &[0x80, 0, 0x13]),
    ];
    for (opcode, params) in allowed {
        assert_eq!(g.check(&command(opcode, params)), Ok(()), "{opcode:#06x}");
        // One byte more or less is refused.
        let mut longer = params.to_vec();
        longer.push(0);
        if longer.len() <= 255 {
            assert_eq!(
                g.check(&command(opcode, &longer)),
                Err(Refusal::BadParams(opcode)),
                "{opcode:#06x} longer"
            );
        }
        if !params.is_empty() {
            assert_eq!(
                g.check(&command(opcode, &params[1..])),
                Err(Refusal::BadParams(opcode)),
                "{opcode:#06x} shorter"
            );
        }
    }
}

#[test]
fn the_gate_checks_parameters_and_lengths() {
    assert_eq!(
        gate(&command(op::RESET, &[0])),
        Err(Refusal::BadParams(op::RESET))
    );
    assert_eq!(
        gate(&command(op::SET_EVENT_MASK, &[0; 7])),
        Err(Refusal::BadParams(op::SET_EVENT_MASK))
    );
    assert_eq!(
        gate(&command(op::REJECT_CONNECTION_REQUEST, &[0; 6])),
        Err(Refusal::BadParams(op::REJECT_CONNECTION_REQUEST))
    );
    // Header says 0 parameters, one more byte follows.
    assert_eq!(gate(&[0x01, 0x03, 0x0C, 0, 0]), Err(Refusal::BadLength));
    // Header says 1, none follows.
    assert_eq!(gate(&[0x01, 0x03, 0x0C, 1]), Err(Refusal::BadLength));
    assert_eq!(gate(&[0x01, 0x03, 0x0C]), Err(Refusal::BadLength));
}

#[test]
fn the_gate_takes_acl_data_of_the_right_length_only() {
    assert_eq!(gate(&[0x02, 0x80, 0x20, 0x01, 0x00, 0xAA]), Ok(()));
    assert_eq!(
        gate(&[0x02, 0x80, 0x20, 0x02, 0x00, 0xAA]),
        Err(Refusal::BadLength)
    );
    assert_eq!(
        gate(&[0x02, 0x80, 0x20, 0x00, 0x00, 0xAA]),
        Err(Refusal::BadLength)
    );
    assert_eq!(gate(&[0x02, 0x80]), Err(Refusal::BadLength));
}

#[test]
fn the_gate_refuses_anything_not_a_command_or_data() {
    assert_eq!(gate(&[]), Err(Refusal::NotACommand));
    assert_eq!(gate(&[0x04, 0x0E, 0]), Err(Refusal::NotACommand));
    assert_eq!(gate(&[0x03, 0x01, 0x00, 0]), Err(Refusal::NotACommand));
    assert_eq!(gate(&[0x05, 0x01]), Err(Refusal::NotACommand));
}

// ---- events -------------------------------------------------------------

#[test]
fn command_complete_and_status_are_read() {
    let p = complete(op::READ_LOCAL_VERSION, 5, &[0, 7, 0x0E, 0x03]);
    assert_eq!(
        parse_event(&p),
        Some(Event::CommandComplete {
            credits: 5,
            opcode: op::READ_LOCAL_VERSION,
            ret: &[0, 7, 0x0E, 0x03]
        })
    );
    let p = event(0x0F, &[0x0C, 1, 0x0A, 0x04]);
    assert_eq!(
        parse_event(&p),
        Some(Event::CommandStatus {
            status: 0x0C,
            credits: 1,
            opcode: op::REJECT_CONNECTION_REQUEST
        })
    );
}

#[test]
fn a_connection_request_is_read_and_answered_no() {
    let mut params = ADDR.to_vec();
    params.extend_from_slice(&[0x08, 0x25, 0x00, 0x01]);
    let p = event(0x04, &params);
    let Some(Event::ConnectionRequest {
        addr,
        class,
        link_type,
    }) = parse_event(&p)
    else {
        panic!("not read");
    };
    assert_eq!(addr.bytes(), &ADDR);
    assert_eq!(class, 0x002508);
    assert_eq!(link_type, 1);
    let answer = answer_connection_request(&addr);
    assert_eq!(answer.opcode(), op::REJECT_CONNECTION_REQUEST);
    assert_eq!(&answer.as_bytes()[4..10], &ADDR);
    assert_eq!(answer.as_bytes()[10], hci_rules::REJECT_UNACCEPTABLE_ADDR);
    assert_eq!(gate(answer.as_bytes()), Ok(()));
}

#[test]
fn connection_complete_and_disconnection_are_read() {
    let mut params = vec![0x0F, 0x80, 0x00];
    params.extend_from_slice(&ADDR);
    params.extend_from_slice(&[0x01, 0x00]);
    assert_eq!(
        parse_event(&event(0x03, &params)),
        Some(Event::ConnectionComplete {
            status: 0x0F,
            handle: 0x80,
            addr: BdAddr::new(ADDR),
            link_type: 1
        })
    );
    assert_eq!(
        parse_event(&event(0x05, &[0, 0x80, 0x00, 0x13])),
        Some(Event::DisconnectionComplete {
            status: 0,
            handle: 0x80,
            reason: 0x13
        })
    );
    assert_eq!(
        parse_event(&event(0x10, &[0x2A])),
        Some(Event::HardwareError { code: 0x2A })
    );
    assert_eq!(
        parse_event(&event(0x17, &ADDR)),
        Some(Event::Other { code: 0x17 })
    );
}

#[test]
fn broken_events_are_not_read() {
    // Length byte does not match.
    assert_eq!(parse_event(&[0x04, 0x0E, 4, 1, 1, 0x10]), None);
    // Command complete too short for its code.
    assert_eq!(parse_event(&event(0x0E, &[1, 0x01])), None);
    // Known events with the wrong length.
    assert_eq!(parse_event(&event(0x04, &ADDR)), None);
    assert_eq!(parse_event(&event(0x03, &[0; 10])), None);
    assert_eq!(parse_event(&event(0x0F, &[0; 3])), None);
    assert_eq!(parse_event(&event(0x05, &[0; 5])), None);
    assert_eq!(parse_event(&event(0x10, &[])), None);
    // Not an event.
    assert_eq!(parse_event(&[0x02, 0x01, 0x20, 0x00, 0x00]), None);
    assert_eq!(parse_event(&[]), None);
}

// ---- addresses never print in full ------------------------------------------

#[test]
fn an_address_prints_only_its_last_two_bytes() {
    let a = BdAddr::new(ADDR);
    assert_eq!(a.tail(), [0xEE, 0xF1]);
    assert_eq!(format!("{a}"), "..ee:f1");
    assert_eq!(format!("{a:?}"), "..ee:f1");
    let mut params = ADDR.to_vec();
    params.extend_from_slice(&[0x08, 0x25, 0x00, 0x01]);
    let p = event(0x04, &params);
    let text = format!("{:?}", parse_event(&p).unwrap());
    // Neither in hex nor as the numbers a plain Debug of the bytes shows.
    for b in ["aa:", "bb:", "cc:", "dd:", "170", "187", "204", "221"] {
        assert!(!text.to_lowercase().contains(b), "{text}");
    }
    assert!(text.contains("..ee:f1"), "{text}");
}

// ---- start-up sequence --------------------------------------------------

/// Runs the sequence against a controller that answers every command
/// with `status_for(opcode)`, and returns the commands it sent.
fn run_setup(status_for: impl Fn(u16) -> u8) -> (Setup, Vec<u16>) {
    let mut s = Setup::new();
    let mut sent = Vec::new();
    for _ in 0..20 {
        let Some(c) = s.due() else { break };
        assert_eq!(gate(c.as_bytes()), Ok(()));
        // Nothing else is due until the answer comes.
        s.sent();
        assert_eq!(s.due(), None);
        sent.push(c.opcode());
        let ans = complete(c.opcode(), 1, &[status_for(c.opcode())]);
        assert!(s.on_event(&parse_event(&ans).unwrap()));
    }
    (s, sent)
}

#[test]
fn setup_sends_the_five_commands_in_order_and_ends_with_page_scan() {
    let (s, sent) = run_setup(|_| 0);
    assert_eq!(sent, Setup::COMMANDS.to_vec());
    assert_eq!(s.step(), Step::Done);
    assert!(s.done());
    assert_eq!(s.due(), None);
}

#[test]
fn setup_stops_at_a_failed_command_and_never_turns_page_scan_on() {
    let (s, sent) = run_setup(|op| if op == op::SET_EVENT_MASK { 0x12 } else { 0 });
    assert_eq!(
        sent,
        vec![
            op::RESET,
            op::READ_LOCAL_VERSION,
            op::READ_BUFFER_SIZE,
            op::SET_EVENT_MASK
        ]
    );
    assert_eq!(
        s.step(),
        Step::Failed {
            opcode: op::SET_EVENT_MASK,
            status: 0x12
        }
    );
    assert_eq!(s.due(), None);
}

#[test]
fn setup_waits_for_credits() {
    let mut s = Setup::new();
    let c = s.due().unwrap();
    s.sent();
    // The answer gives no credit back: the next command waits.
    assert!(s.on_event(&parse_event(&complete(c.opcode(), 0, &[0])).unwrap()));
    assert_eq!(s.step(), Step::Send(1));
    assert_eq!(s.due(), None);
    // Any later event with credits frees it.
    let other = complete(op::READ_BUFFER_SIZE, 3, &[0]);
    assert!(!s.on_event(&parse_event(&other).unwrap()));
    assert_eq!(s.credits(), 3);
    assert_eq!(s.due().unwrap().opcode(), op::READ_LOCAL_VERSION);
}

#[test]
fn setup_ignores_answers_to_other_commands() {
    let mut s = Setup::new();
    s.due().unwrap();
    s.sent();
    let other = complete(op::READ_LOCAL_VERSION, 1, &[0]);
    assert!(!s.on_event(&parse_event(&other).unwrap()));
    assert_eq!(s.step(), Step::Waiting(0));
    // A Command Status that takes a command changes nothing either.
    let st = event(0x0F, &[0, 1, 0x03, 0x0C]);
    assert!(!s.on_event(&parse_event(&st).unwrap()));
    assert_eq!(s.step(), Step::Waiting(0));
}

#[test]
fn setup_stops_when_its_command_is_refused_by_status() {
    let mut s = Setup::new();
    s.due().unwrap();
    s.sent();
    let st = event(0x0F, &[0x01, 1, 0x03, 0x0C]);
    assert!(s.on_event(&parse_event(&st).unwrap()));
    assert_eq!(
        s.step(),
        Step::Failed {
            opcode: op::RESET,
            status: 0x01
        }
    );
}

#[test]
fn setup_stops_on_a_hardware_error() {
    let mut s = Setup::new();
    s.due().unwrap();
    s.sent();
    assert!(!s.on_event(&parse_event(&event(0x10, &[3])).unwrap()));
    assert_eq!(
        s.step(),
        Step::Failed {
            opcode: op::RESET,
            status: 0xFF
        }
    );
    // After Done, a hardware error does not undo it.
    let (mut done, _) = run_setup(|_| 0);
    done.on_event(&parse_event(&event(0x10, &[3])).unwrap());
    assert!(done.done());
}

#[test]
fn spending_a_credit_outside_the_sequence_counts() {
    let mut s = Setup::new();
    s.spend_credit();
    assert_eq!(s.credits(), 0);
    assert_eq!(s.due(), None);
    s.spend_credit();
    assert_eq!(s.credits(), 0);
}

#[test]
fn setup_stops_for_good_when_an_answer_does_not_come() {
    let mut s = Setup::new();
    s.due().unwrap();
    s.sent();
    s.time_out();
    assert_eq!(
        s.step(),
        Step::Failed {
            opcode: op::RESET,
            status: 0xFE
        }
    );
    assert_eq!(s.due(), None);
    // A late answer changes nothing.
    assert!(!s.on_event(&parse_event(&complete(op::RESET, 1, &[0])).unwrap()));
    assert_eq!(s.due(), None);
    // After Done, a time-out does not undo it.
    let (mut done, _) = run_setup(|_| 0);
    done.time_out();
    assert!(done.done());
}

// ---- the supervision timeout: only on a saved pad's link -----------------

/// Connection Complete (status, handle, address, ACL).
fn conn_complete(status: u8, handle: u16, addr: [u8; 6], link_type: u8) -> Vec<u8> {
    let mut params = vec![status];
    params.extend_from_slice(&handle.to_le_bytes());
    params.extend_from_slice(&addr);
    params.extend_from_slice(&[link_type, 0x00]);
    event(0x03, &params)
}

fn lsto(handle: u16, slots: u16) -> Vec<u8> {
    let h = handle.to_le_bytes();
    let v = slots.to_le_bytes();
    command(
        op::WRITE_LINK_SUPERVISION_TIMEOUT,
        &[h[0], h[1], v[0], v[1]],
    )
}

#[test]
fn the_supervision_timeout_goes_only_to_a_saved_pads_link() {
    let other = [0x9A, 0xE7, 0x5C, 0xB1, 0xD3, 0x4F];
    let mut g = gate_for(&[ADDR]);
    let nope = Err(Refusal::NotSaved(op::WRITE_LINK_SUPERVISION_TIMEOUT));
    // No link yet: refused for any handle.
    assert_eq!(g.check(&lsto(0x80, 0x0C80)), nope);
    // A link to a device that is not saved: still refused.
    g.observe(&conn_complete(0, 0x81, other, 1));
    assert_eq!(g.check(&lsto(0x81, 0x0C80)), nope);
    // A failed connection to the saved pad: nothing noted.
    g.observe(&conn_complete(0x04, 0x80, ADDR, 1));
    assert_eq!(g.check(&lsto(0x80, 0x0C80)), nope);
    // A voice link (SCO, type 0) to the saved pad: not an ACL link.
    g.observe(&conn_complete(0, 0x82, ADDR, 0));
    assert_eq!(g.check(&lsto(0x82, 0x0C80)), nope);
    // The saved pad's link is up: allowed on its handle only.
    g.observe(&conn_complete(0, 0x80, ADDR, 1));
    assert_eq!(g.linked(), 1);
    assert_eq!(g.check(&lsto(0x80, 0x0C80)), Ok(()));
    assert_eq!(g.check(&lsto(0x81, 0x0C80)), nope);
    assert_eq!(g.check(&lsto(0x1080, 0x0C80)), nope, "reserved bits set");
    assert_eq!(format!("{g:?}"), "Gate(1 saved, 1 linked)");
    // 1 s at least; 0 (no timeout at all) never; 4 bytes exactly.
    assert_eq!(g.check(&lsto(0x80, 0x0640)), Ok(()));
    assert_eq!(g.check(&lsto(0x80, 0xFFFF)), Ok(()));
    let bad = Err(Refusal::BadParams(op::WRITE_LINK_SUPERVISION_TIMEOUT));
    assert_eq!(g.check(&lsto(0x80, 0x063F)), bad);
    assert_eq!(g.check(&lsto(0x80, 0)), bad);
    assert_eq!(
        g.check(&command(
            op::WRITE_LINK_SUPERVISION_TIMEOUT,
            &[0x80, 0, 0x80]
        )),
        bad
    );
    // Another link's end changes nothing; its own end closes the door.
    g.observe(&event(0x05, &[0, 0x81, 0x00, 0x13]));
    assert_eq!(g.check(&lsto(0x80, 0x0C80)), Ok(()));
    g.observe(&event(0x05, &[0x0C, 0x80, 0x00, 0x13]));
    assert_eq!(g.check(&lsto(0x80, 0x0C80)), Ok(()), "a failed disconnect");
    g.observe(&event(0x05, &[0, 0x80, 0x00, 0x08]));
    assert_eq!(g.linked(), 0);
    assert_eq!(g.check(&lsto(0x80, 0x0C80)), nope);
    // Without a saved pad the gate never notes a link.
    let mut none = Gate::new();
    none.observe(&conn_complete(0, 0x80, ADDR, 1));
    assert_eq!(none.check(&lsto(0x80, 0x0C80)), nope);
    // Commands, data and broken events change nothing.
    let mut g2 = gate_for(&[ADDR]);
    g2.observe(&lsto(0x80, 0x0C80));
    g2.observe(&[0x04, 0x03, 0x0B, 0, 0x80]);
    g2.observe(&[]);
    assert_eq!(g2.linked(), 0);
}

#[test]
fn the_gate_notes_at_most_four_links_and_each_handle_once() {
    let mut g = gate_for(&[ADDR]);
    let nope = Err(Refusal::NotSaved(op::WRITE_LINK_SUPERVISION_TIMEOUT));
    for _ in 0..3 {
        g.observe(&conn_complete(0, 0x80, ADDR, 1));
    }
    assert_eq!(g.linked(), 1, "the same handle twice is one link");
    for h in 0x81..0x88 {
        g.observe(&conn_complete(0, h, ADDR, 1));
    }
    // Full (their ends never came): the newest four are kept.
    assert_eq!(g.linked(), 4);
    for h in 0x84..0x88 {
        assert_eq!(g.check(&lsto(h, 0x0C80)), Ok(()), "{h:#x}");
    }
    for h in 0x80..0x84 {
        assert_eq!(g.check(&lsto(h, 0x0C80)), nope, "{h:#x}");
    }
    g.observe(&event(0x05, &[0, 0x85, 0x00, 0x08]));
    assert_eq!(g.linked(), 3);
    g.observe(&conn_complete(0, 0x80, ADDR, 1));
    assert_eq!(g.check(&lsto(0x80, 0x0C80)), Ok(()));
    assert_eq!(
        g.check(&lsto(0x84, 0x0C80)),
        Ok(()),
        "a free slot: nothing forgotten"
    );
}
