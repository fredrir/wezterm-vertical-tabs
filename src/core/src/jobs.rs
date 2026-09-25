//! Shell-owned jobs. The pane transports snapshots and requests without executing commands.
use crate::PaneId;

pub const JOBS_VAR: &str = "vtabs_jobs";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobOperation {
    Foreground,
    Background,
    Terminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobTarget {
    pub pane: PaneId,
    pub shell: u32,
    pub number: u32,
    pub pid: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub number: u32,
    pub pid: u32,
    pub suspended: bool,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellJobs {
    pub shell: u32,
    pub generation: u64,
    pub ready: bool,
    pub host: String,
    pub jobs: Vec<Job>,
}

impl ShellJobs {
    /// Versioned TSV inside an OSC user variable. Shell text has controls removed.
    pub fn parse(value: &str) -> Option<Self> {
        if value.len() > 256 * 1024 {
            return None;
        }
        let mut lines = value.lines();
        let mut header = lines.next()?.split('\t');
        if header.next()? != "1" {
            return None;
        }
        let shell = positive(header.next()?)?;
        let generation = header.next()?.parse().ok()?;
        let ready = match header.next()? {
            "0" => false,
            "1" => true,
            _ => return None,
        };
        let host = printable(header.next()?)?.to_owned();
        if header.next().is_some() {
            return None;
        }
        let mut jobs = Vec::new();
        for line in lines {
            if jobs.len() == 128 {
                return None;
            }
            let mut fields = line.splitn(4, '\t');
            let number = positive(fields.next()?)?;
            let pid = positive(fields.next()?)?;
            let suspended = match fields.next()? {
                "running" => false,
                "suspended" => true,
                _ => return None,
            };
            let command = printable(fields.next()?)?.to_owned();
            if jobs.iter().any(|job: &Job| job.number == number) {
                return None;
            }
            jobs.push(Job {
                number,
                pid,
                suspended,
                command,
            });
        }
        Some(Self {
            shell,
            generation,
            ready,
            host,
            jobs,
        })
    }

    pub fn request(
        &self,
        target: JobTarget,
        operation: JobOperation,
    ) -> Result<String, &'static str> {
        if self.shell != target.shell
            || !self
                .jobs
                .iter()
                .any(|job| job.number == target.number && job.pid == target.pid)
        {
            return Err("Job no longer exists");
        }
        if !self.ready {
            return Err("Return to the job's shell prompt first");
        }
        Ok(self.sequence(
            target.number,
            target.pid,
            match operation {
                JobOperation::Foreground => 1,
                JobOperation::Background => 2,
                JobOperation::Terminate => 3,
            },
        ))
    }

    pub fn refresh(&self) -> Option<String> {
        self.ready.then(|| self.sequence(0, 0, 0))
    }

    fn sequence(&self, number: u32, pid: u32, operation: u8) -> String {
        format!(
            "\x1b[777;{};{};{number};{pid};{operation}~",
            self.shell, self.generation
        )
    }
}

fn positive(value: &str) -> Option<u32> {
    value.parse().ok().filter(|value| *value > 0)
}

fn printable(value: &str) -> Option<&str> {
    (!value.chars().any(char::is_control)).then_some(value)
}

#[cfg(test)]
#[path = "../tests/jobs.rs"]
mod tests;
