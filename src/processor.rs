use crate::parse_error;
use std::io::{Read, Write};
use anyhow::Result;

#[derive(PartialEq, Clone, Copy, Hash)]
pub enum ProcessingType {
    /// Clean filter is ran on stage
    Clean,
    /// Smudge filter is ran on checkout
    Smudge,
}

impl ProcessingType {
    pub fn name(&self) -> &'static str {
        match self {
            ProcessingType::Clean => "clean",
            ProcessingType::Smudge => "smudge",
        }
    }
    pub fn done_name(&self) -> &'static str {
        match self {
            ProcessingType::Clean => "cleaned",
            ProcessingType::Smudge => "smudged",
        }
    }
    pub fn acc_name(&self) -> &'static str {
        match self {
            Self::Clean => "cleaning",
            Self::Smudge => "smudging",
        }
    }
}

/// This trait is used for user-defined logic of git-filter-server
/// Typically git talks with processor via stdio, so better do not use it inside
pub trait Processor {
    /// Handle clean/smudge operation
    fn process<R: Read, W: Write>(
        &mut self,
        _pathname: &str,
        _process_type: ProcessingType,
        _input: &mut R,
        _output: &mut W,
    ) -> Result<()> {
        Err(parse_error!("processing is not supported").into())
    }

    /// Schedule delayed execution
    fn schedule_process<R: Read>(
        &mut self,
        _pathname: &str,
        _process_type: ProcessingType,
        _input: &mut R,
    ) -> Result<()> {
        panic!("delayed processing is not implemented")
    }

    /// Get data for file, previously scheduled via schedule_process
    fn get_scheduled<W: Write>(
        &mut self,
        _pathname: &str,
        _process_type: ProcessingType,
        _output: &mut W,
    ) -> Result<()> {
        panic!("delayed processing is not implemented")
    }
    /// Called once all files are already scheduled/processed
    fn switch_to_wait(&mut self) {}

    /// Get scheduled files ready for outputting
    fn get_available(&mut self) -> Result<Vec<String>> {
        panic!("delayed processing is not implemented")
    }

    /// Should processing of file be delayed?
    /// Only use it for long-running tasks, i.e file downloading, which would be better parallelized
    fn should_delay(&self, _pathname: &str, _process_type: ProcessingType) -> bool {
        false
    }

    /// Does this filter supports clean/smudge?
    fn supports_processing(&self, _process_type: ProcessingType) -> bool {
        false
    }
}

// Noop processor
impl Processor for () {}
