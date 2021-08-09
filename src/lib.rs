use std::io::{ErrorKind, Read, Result, Write};

use ext::{ReadExt, WriteExt};

use tracing::{error, info_span};
use util::{ReadPktUntilFlush, WritePkt};
pub(crate) mod ext;
mod processor;
mod util;
pub use processor::*;

#[macro_export]
macro_rules! parse_error {
    ($e:expr) => {
        std::io::Error::new(std::io::ErrorKind::InvalidData, $e)
    };
}

pub struct GitFilterServer<P>(P);

impl<P> GitFilterServer<P> {
    pub fn new(processor: P) -> Self {
        Self(processor)
    }
}

impl<P: Processor> GitFilterServer<P> {
    fn communicate_internal<R: Read, W: Write>(
        &mut self,
        mut input: &mut R,
        mut output: &mut W,
    ) -> Result<()> {
        let mut buf = Vec::new();
        {
            if input.pkt_text_read(&mut buf)? != Some("git-filter-client") {
                return Err(parse_error!("bad prelude").into());
            }
            if input.pkt_text_read(&mut buf)? != Some("version=2") {
                return Err(parse_error!("unknown version").into());
            }
            if input.pkt_text_read(&mut buf)? != None {
                return Err(parse_error!("unexpected text after client hello").into());
            }
        }
        {
            output.pkt_text_write("git-filter-server")?;
            output.pkt_text_write("version=2")?;
            output.pkt_end()?;
        }
        {
            let mut filter = false;
            let mut smudge = false;
            let mut delay = false;
            while let Some(command) = input.pkt_text_read(&mut buf)? {
                match command {
                    "capability=clean" => filter = true,
                    "capability=smudge" => smudge = true,
                    "capability=delay" => delay = true,
                    _ => {}
                }
            }
            if filter && self.0.supports_processing(ProcessingType::Clean) {
                output.pkt_text_write("capability=clean")?;
            }
            if smudge && self.0.supports_processing(ProcessingType::Smudge) {
                output.pkt_text_write("capability=smudge")?;
            }
            if delay {
                output.pkt_text_write("capability=delay")?;
            }
            output.pkt_end()?;
        }

        let mut waiting_for_blobs = false;
        loop {
            let mut command = None;
            let mut pathname = None;
            let mut can_delay = false;
            while let Some(input) = input.pkt_text_read(&mut buf)? {
                if let Some(command_val) = input.strip_prefix("command=") {
                    command = Some(command_val.to_owned());
                } else if let Some(pathname_val) = input.strip_prefix("pathname=") {
                    pathname = Some(pathname_val.to_owned())
                } else if input == "can-delay=1" {
                    can_delay = true;
                }
            }
            let command = command.ok_or(parse_error!("missing command"))?;
            let _span = info_span!("command", command = format_args!("{:?}", command),).entered();

            match command.as_str() {
                t @ "clean" | t @ "smudge" => {
                    let process_type = match t {
                        "clean" => ProcessingType::Clean,
                        "smudge" => ProcessingType::Smudge,
                        _ => unreachable!(),
                    };
                    let pathname = pathname.ok_or(parse_error!("missing pathname"))?;
                    let mut process_input = ReadPktUntilFlush::new(&mut input);
                    if waiting_for_blobs {
                        let _span = info_span!(
                            "resolving delayed",
                            pathname = format_args!("{}", pathname)
                        )
                        .entered();
                        let mut sink = [0; 1];
                        process_input
                            .read_exact(&mut sink)
                            .map_err(|_| parse_error!("delayed blob should have no data"))?;
                        assert!(process_input.finished());

                        output.pkt_text_write("status=success")?;
                        output.pkt_end()?;
                        let mut process_output = WritePkt::new(&mut output);
                        if let Err(e) =
                            self.0
                                .get_scheduled(&pathname, process_type, &mut process_output)
                        {
                            process_output.flush()?;
                            drop(process_output);
                            error!("{:#}", e);
                            output.pkt_end()?;
                            output.pkt_text_write("status=error")?;
                            output.pkt_end()?;
                            return Ok(());
                        } else {
                            process_output.flush()?;
                            drop(process_output);
                            output.pkt_end()?;
                            // Keep status
                            output.pkt_end()?;
                        }
                    } else if can_delay && self.0.should_delay(&pathname, process_type) {
                        let _span =
                            info_span!("scheduling", pathname = format_args!("{}", pathname))
                                .entered();
                        if let Err(e) =
                            self.0
                                .schedule_process(&pathname, process_type, &mut process_input)
                        {
                            error!("{:#}", e);
                            output.pkt_text_write("status=error")?;
                            output.pkt_end()?;
                            return Ok(());
                        } else {
                            output.pkt_text_write("status=delayed")?;
                            output.pkt_end()?;
                        }
                    } else {
                        let _span =
                            info_span!("processing", pathname = format_args!("{}", pathname))
                                .entered();
                        output.pkt_text_write("status=success")?;
                        output.pkt_end()?;
                        let mut process_output = WritePkt::new(&mut output);
                        if let Err(e) = self.0.process(
                            &pathname,
                            process_type,
                            &mut process_input,
                            &mut process_output,
                        ) {
                            process_output.flush()?;
                            drop(process_output);
                            error!("{:#}", e);
                            output.pkt_end()?;
                            output.pkt_text_write("status=error")?;
                            output.pkt_end()?;
                            return Ok(());
                        } else {
                            process_output.flush()?;
                            drop(process_output);
                            output.pkt_end()?;
                            // Keep status
                            output.pkt_end()?;
                        }
                    }
                    // Input should be stopped at flush
                    assert!(process_input.finished());
                }
                "list_available_blobs" => {
                    self.0.switch_to_wait();
                    waiting_for_blobs = true;
                }
                cmd => return Err(parse_error!(format!("unknown command: {}", cmd)).into()),
            }
        }
    }

    pub fn communicate<R: Read, W: Write>(&mut self, input: &mut R, output: &mut W) -> Result<()> {
        match self.communicate_internal(input, output) {
            Ok(_) => Ok(()),
            // Communication is done, not a error
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn communicate_stdio(&mut self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();

        self.communicate(&mut stdin.lock(), &mut stdout.lock())?;
        Ok(())
    }
}
