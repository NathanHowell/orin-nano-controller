//! Adapters that bridge firmware data structures with `controller-core`.
//!
//! The MCU crate currently owns the concrete queue, sequence, and telemetry
//! types that will eventually be driven by the shared `controller-core`
//! orchestrator. This module provides conversion helpers plus light-weight
//! wrappers that implement the controller traits without forcing the rest of
//! the firmware to restructure immediately.

#![allow(dead_code)]

use controller_core::{
    orchestrator::{
        CommandDequeueError as CoreCommandDequeueError,
        CommandEnqueueError as CoreCommandEnqueueError, CommandFlags as CoreCommandFlags,
        CommandQueueConsumer as CoreCommandQueueConsumer,
        CommandQueueProducer as CoreCommandQueueProducer, CommandSource as CoreCommandSource,
        SequenceCommand as CoreSequenceCommand,
    },
    sequences::StrapSequenceKind as CoreStrapSequenceKind,
};
use embassy_sync::channel::{TryReceiveError, TrySendError};
use embassy_time::Instant;

use crate::straps;

/// Adapter that allows the firmware command sender to satisfy the
/// `controller-core` queue producer trait.
pub struct CommandProducer<'a> {
    sender: straps::CommandSender<'a>,
}

impl<'a> CommandProducer<'a> {
    /// Creates a new adapter that wraps the firmware sender.
    pub fn new(sender: straps::CommandSender<'a>) -> Self {
        Self { sender }
    }

    /// Provides access to the wrapped sender.
    pub fn inner(&self) -> &straps::CommandSender<'a> {
        &self.sender
    }

    /// Provides mutable access to the wrapped sender.
    pub fn inner_mut(&mut self) -> &mut straps::CommandSender<'a> {
        &mut self.sender
    }

    /// Consumes the adapter and returns the underlying sender.
    pub fn into_inner(self) -> straps::CommandSender<'a> {
        self.sender
    }
}

impl<'a> CoreCommandQueueProducer for CommandProducer<'a> {
    type Instant = Instant;
    type Error = TrySendError<straps::SequenceCommand>;

    fn try_enqueue(
        &mut self,
        command: CoreSequenceCommand<Self::Instant>,
    ) -> Result<(), CoreCommandEnqueueError<Self::Error>> {
        match self.sender.try_send(into_firmware_command(command)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(CoreCommandEnqueueError::QueueFull),
        }
    }

    fn capacity(&self) -> Option<usize> {
        Some(straps::COMMAND_QUEUE_DEPTH)
    }
}

/// Adapter that allows the firmware command receiver to satisfy the
/// `controller-core` queue consumer trait.
pub struct CommandConsumer<'a> {
    receiver: straps::CommandReceiver<'a>,
}

impl<'a> CommandConsumer<'a> {
    /// Creates a new adapter that wraps the firmware receiver.
    pub fn new(receiver: straps::CommandReceiver<'a>) -> Self {
        Self { receiver }
    }

    /// Provides access to the wrapped receiver.
    pub fn inner(&self) -> &straps::CommandReceiver<'a> {
        &self.receiver
    }

    /// Provides mutable access to the wrapped receiver.
    pub fn inner_mut(&mut self) -> &mut straps::CommandReceiver<'a> {
        &mut self.receiver
    }

    /// Consumes the adapter and returns the underlying receiver.
    pub fn into_inner(self) -> straps::CommandReceiver<'a> {
        self.receiver
    }
}

impl<'a> CoreCommandQueueConsumer for CommandConsumer<'a> {
    type Instant = Instant;
    type Error = TryReceiveError;

    fn try_dequeue(
        &mut self,
    ) -> Result<Option<CoreSequenceCommand<Self::Instant>>, CoreCommandDequeueError<Self::Error>>
    {
        match self.receiver.try_receive() {
            Ok(command) => Ok(Some(into_core_command(command))),
            Err(TryReceiveError::Empty) => Ok(None),
        }
    }
}

fn into_firmware_command(command: CoreSequenceCommand<Instant>) -> straps::SequenceCommand {
    let CoreSequenceCommand {
        kind,
        requested_at,
        source,
        flags,
    } = command;

    let mut converted = straps::SequenceCommand::new(
        into_firmware_kind(kind),
        requested_at,
        into_firmware_source(source),
    );
    converted.flags = into_firmware_flags(flags);
    converted
}

fn into_core_command(command: straps::SequenceCommand) -> CoreSequenceCommand<Instant> {
    let straps::SequenceCommand {
        kind,
        requested_at,
        source,
        flags,
    } = command;

    CoreSequenceCommand::with_flags(
        into_core_kind(kind),
        requested_at,
        into_core_source(source),
        into_core_flags(flags),
    )
}

fn into_firmware_kind(kind: CoreStrapSequenceKind) -> straps::StrapSequenceKind {
    match kind {
        CoreStrapSequenceKind::NormalReboot => straps::StrapSequenceKind::NormalReboot,
        CoreStrapSequenceKind::RecoveryEntry => straps::StrapSequenceKind::RecoveryEntry,
        CoreStrapSequenceKind::RecoveryImmediate => straps::StrapSequenceKind::RecoveryImmediate,
        CoreStrapSequenceKind::FaultRecovery => straps::StrapSequenceKind::FaultRecovery,
    }
}

fn into_core_kind(kind: straps::StrapSequenceKind) -> CoreStrapSequenceKind {
    match kind {
        straps::StrapSequenceKind::NormalReboot => CoreStrapSequenceKind::NormalReboot,
        straps::StrapSequenceKind::RecoveryEntry => CoreStrapSequenceKind::RecoveryEntry,
        straps::StrapSequenceKind::RecoveryImmediate => CoreStrapSequenceKind::RecoveryImmediate,
        straps::StrapSequenceKind::FaultRecovery => CoreStrapSequenceKind::FaultRecovery,
    }
}

fn into_firmware_source(source: CoreCommandSource) -> straps::CommandSource {
    match source {
        CoreCommandSource::UsbHost => straps::CommandSource::UsbHost,
    }
}

fn into_core_source(source: straps::CommandSource) -> CoreCommandSource {
    match source {
        straps::CommandSource::UsbHost => CoreCommandSource::UsbHost,
    }
}

fn into_firmware_flags(flags: CoreCommandFlags) -> straps::CommandFlags {
    straps::CommandFlags {
        force_recovery: flags.force_recovery,
        // Delay semantics live solely in the firmware layer today; carry the
        // value once the shared core grows a compatible field.
        start_after: None,
    }
}

fn into_core_flags(flags: straps::CommandFlags) -> CoreCommandFlags {
    CoreCommandFlags {
        force_recovery: flags.force_recovery,
    }
}
