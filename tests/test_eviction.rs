// Copyright (C) 2025 Matías Salinas (support@fenden.com)
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#[cfg(test)]
mod tests {
    use super::*;
    use cachebolt::{eviction::{start_background_eviction_task, start_background_eviction_task_with}, memory::memory::{maybe_evict_if_needed, MEMORY_CACHE}};
    use std::sync::{Arc, Mutex};
    use tokio::time::{self, Duration};
    use tokio::task;


    #[tokio::test]
    async fn test_eviction_triggered_on_increased_memory() {
        time::pause();

        let usage_sequence = Arc::new(Mutex::new(vec![
            (400, 1000), // 40%
            (600, 1000), // 60% → should trigger eviction
            (600, 1000), // 60%
        ]));

        let sequence = usage_sequence.clone();

        let get_mocked = move || {
            let mut seq = sequence.lock().unwrap();
            seq.remove(0)
        };

        start_background_eviction_task_with(get_mocked);
        time::advance(Duration::from_secs(3)).await;
    }

    #[tokio::test]
    async fn test_start_background_eviction_task_runs() {
        start_background_eviction_task();
        tokio::time::sleep(Duration::from_millis(1100)).await;
    }

    #[tokio::test]
    async fn test_eviction_logic_runs() {
        time::pause();

        let usage_sequence = Arc::new(Mutex::new(vec![
            (100, 1000), // 10%
            (900, 1000), // 90% → trigger
        ]));

        let usage_clone = usage_sequence.clone();
        let triggered = Arc::new(tokio::sync::Notify::new());
        let triggered_clone = triggered.clone();

        let get_mocked = move || {
            let mut seq = usage_clone.lock().unwrap();
            if seq.is_empty() {
                return (1000, 1000);
            }
            seq.remove(0)
        };

        let _ = task::spawn({
            let triggered = triggered_clone;
            async move {
                start_background_eviction_task_with(get_mocked);
                triggered.notify_one();
            }
        });

        triggered.notified().await;
        time::advance(Duration::from_secs(2)).await;
    }
}
