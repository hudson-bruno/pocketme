//! The start-up sequence: one command at a time, each sent only after
//! the one before it finished well. A command that fails (or a fault in
//! the controller) stops the sequence where it is, so page scan is only
//! turned on after everything before it worked.

use crate::{Cmd, EVENT_MASK, Event, op};

/// Where the sequence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The next command is due (index into [`Setup::COMMANDS`]).
    Send(u8),
    /// Sent, waiting for its answer.
    Waiting(u8),
    /// All done: page scan is on.
    Done,
    /// Stopped: this command answered with this status (`opcode`, `status`;
    /// status 0xFF for a controller fault, 0xFE for no answer in time).
    Failed { opcode: u16, status: u8 },
}

/// The start-up sequence, and the controller's command credits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    step: Step,
    /// How many commands the controller can take now (it says so in each
    /// Command Complete and Command Status). One at start.
    credits: u8,
}

impl Default for Setup {
    fn default() -> Self {
        Self::new()
    }
}

impl Setup {
    /// The commands, in order.
    pub const COMMANDS: [u16; 5] = [
        op::RESET,
        op::READ_LOCAL_VERSION,
        op::READ_BUFFER_SIZE,
        op::SET_EVENT_MASK,
        op::WRITE_SCAN_ENABLE,
    ];

    pub const fn new() -> Self {
        Setup {
            step: Step::Send(0),
            credits: 1,
        }
    }

    pub fn step(&self) -> Step {
        self.step
    }

    pub fn done(&self) -> bool {
        self.step == Step::Done
    }

    /// How many commands the controller can take now.
    pub fn credits(&self) -> u8 {
        self.credits
    }

    /// One command sent outside the sequence (it used a credit).
    pub fn spend_credit(&mut self) {
        self.credits = self.credits.saturating_sub(1);
    }

    /// The command due now, if one is and the controller can take it.
    /// Call [`Setup::sent`] once it went out.
    pub fn due(&self) -> Option<Cmd> {
        let Step::Send(i) = self.step else {
            return None;
        };
        if self.credits == 0 {
            return None;
        }
        Some(match Self::COMMANDS[usize::from(i)] {
            op::RESET => Cmd::reset(),
            op::READ_LOCAL_VERSION => Cmd::read_local_version(),
            op::READ_BUFFER_SIZE => Cmd::read_buffer_size(),
            op::SET_EVENT_MASK => Cmd::set_event_mask(EVENT_MASK),
            _ => Cmd::write_scan_enable(true),
        })
    }

    /// The command from [`Setup::due`] went to the controller.
    pub fn sent(&mut self) {
        if let Step::Send(i) = self.step {
            self.step = Step::Waiting(i);
            self.spend_credit();
        }
    }

    /// The answer to the waiting command did not come in time: stop
    /// (status 0xFE). Nothing more is sent.
    pub fn time_out(&mut self) {
        if let Step::Waiting(i) | Step::Send(i) = self.step {
            self.step = Step::Failed {
                opcode: Self::COMMANDS[usize::from(i)],
                status: 0xFE,
            };
        }
    }

    /// Every event goes through here (credits, answers, faults). Returns
    /// true when the event finished the command the sequence waited for.
    pub fn on_event(&mut self, ev: &Event<'_>) -> bool {
        let (opcode, status) = match *ev {
            Event::CommandComplete {
                credits,
                opcode,
                ret,
            } => {
                self.credits = credits;
                (opcode, ret.first().copied().unwrap_or(0xFF))
            }
            Event::CommandStatus {
                status,
                credits,
                opcode,
            } => {
                self.credits = credits;
                // A command used here always ends with Command Complete;
                // a Command Status for it only matters if it refuses.
                if status == 0 {
                    return false;
                }
                (opcode, status)
            }
            Event::HardwareError { .. } => {
                if !matches!(self.step, Step::Done | Step::Failed { .. }) {
                    let opcode = match self.step {
                        Step::Send(i) | Step::Waiting(i) => Self::COMMANDS[usize::from(i)],
                        _ => 0,
                    };
                    self.step = Step::Failed {
                        opcode,
                        status: 0xFF,
                    };
                }
                return false;
            }
            _ => return false,
        };
        let Step::Waiting(i) = self.step else {
            return false;
        };
        if opcode != Self::COMMANDS[usize::from(i)] {
            return false;
        }
        self.step = if status != 0 {
            Step::Failed { opcode, status }
        } else if usize::from(i) + 1 == Self::COMMANDS.len() {
            Step::Done
        } else {
            Step::Send(i + 1)
        };
        true
    }
}
