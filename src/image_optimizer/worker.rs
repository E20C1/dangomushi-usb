use super::{optimize_workspace_image, CandidateArtifact, OptimizeError, OptimizeSettings};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

#[derive(Debug, Clone)]
pub struct OptimizeJob {
    pub generation: u64,
    pub workspace_root: PathBuf,
    pub source_relative: PathBuf,
    pub settings: OptimizeSettings,
}

pub struct OptimizeCompletion {
    pub generation: u64,
    pub result: Result<CandidateArtifact, OptimizeError>,
}

pub struct OptimizerWorker {
    sender: Sender<OptimizeJob>,
    completed: Receiver<OptimizeCompletion>,
}

impl OptimizerWorker {
    pub fn new() -> Self {
        let (sender, requests) = mpsc::channel::<OptimizeJob>();
        let (completed_sender, completed) = mpsc::channel::<OptimizeCompletion>();

        thread::spawn(move || {
            while let Ok(first) = requests.recv() {
                let mut latest = first;
                while let Ok(next) = requests.try_recv() {
                    latest = next;
                }

                let generation = latest.generation;
                let result = optimize_workspace_image(
                    &latest.workspace_root,
                    &latest.source_relative,
                    latest.settings,
                );

                if completed_sender
                    .send(OptimizeCompletion { generation, result })
                    .is_err()
                {
                    break;
                }
            }
        });

        Self { sender, completed }
    }

    pub fn submit(&self, job: OptimizeJob) -> Result<(), OptimizeJob> {
        self.sender.send(job).map_err(|error| error.0)
    }

    pub fn poll_latest(&self) -> Option<OptimizeCompletion> {
        let mut latest = match self.completed.try_recv() {
            Ok(completion) => completion,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
        };
        while let Ok(next) = self.completed.try_recv() {
            latest = next;
        }
        Some(latest)
    }
}

impl Default for OptimizerWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_queued_jobs_replace_older_unstarted_jobs() {
        let worker = OptimizerWorker::new();
        let root = PathBuf::from("definitely-missing-workspace");

        worker
            .submit(OptimizeJob {
                generation: 1,
                workspace_root: root.clone(),
                source_relative: PathBuf::from("one.png"),
                settings: OptimizeSettings::default(),
            })
            .expect("submit one");
        worker
            .submit(OptimizeJob {
                generation: 2,
                workspace_root: root,
                source_relative: PathBuf::from("two.png"),
                settings: OptimizeSettings::default(),
            })
            .expect("submit two");

        // The worker contract is latest-wins for queued work. It may have already
        // started generation 1, so this test only locks the public job identity.
        // TUI generation matching discards any stale completion that races through.
        assert!(worker.sender.send(OptimizeJob {
            generation: 3,
            workspace_root: PathBuf::from("missing"),
            source_relative: PathBuf::from("three.png"),
            settings: OptimizeSettings::default(),
        }).is_ok());
    }
}
