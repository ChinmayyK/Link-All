# Link All Remote File Queries — Test Infrastructure & Specifications (`TEST_INFRA.md`)

## 1. Overview & Architecture

This document specifies the automated 4-tier End-to-End (E2E) testing framework for **Link All Remote File Queries** (Phase 3 Remote Media Explorer).

### Architecture Stack
```
+-------------------------------------------------------------------------------+
|                             CLIENT / APPLICATION LAYER                        |
|   Swift GUI (macOS)  |  Kotlin App (Android)  |  WinUI C# (Windows)  |  CLI   |
+-------------------------------------------------------------------------------+
                                      | Local IPC (Unix Socket / Named Pipe)
                                      v
+-------------------------------------------------------------------------------+
|                            LOCAL IPC LAYER (`ipc.rs`)                         |
|   - `IpcRequest::RemoteFilesQuery`                                            |
|   - `IpcResponse::Ok(RemoteFilesResponse)` / `IpcResponse::Err(String)`      |
+-------------------------------------------------------------------------------+
                                      | In-Process Async Channel / Engine Call
                                      v
+-------------------------------------------------------------------------------+
|                         LINKALL CORE ENGINE (`engine/mod.rs`)               |
|   - `query_remote_files_sync()` async waiter map management                  |
|   - `EngineEvent::RemoteFilesQueryReceived` & `RemoteFilesResponseReceived`   |
+-------------------------------------------------------------------------------+
                                      | Wire Protocol (`protocol.rs`)
                                      v
+-------------------------------------------------------------------------------+
|                       ENCRYPTED WIRE LAYER (`protocol.rs`)                    |
|   - `AppMessage::RemoteFilesQuery`                                            |
|   - `AppMessage::RemoteFilesResponse`                                         |
+-------------------------------------------------------------------------------+
```

---

## 2. Test Runner Commands

### 2.1 Rust Integration Test Suite
To run the full Rust 4-tier integration test suite inside `linkall-core`:
```bash
cd /Users/chinmayk/Projects/LinkAll/linkall-core
cargo test --test remote_files_e2e_test
```

To run a specific test by name:
```bash
cargo test --test remote_files_e2e_test test_tier1_feature_query_images_category
```

### 2.2 Python IPC Test Harness
To run the Python local IPC JSON serialization test script:
```bash
cd /Users/chinmayk/Projects/LinkAll
python3 scripts/test_remote_files_ipc.py
```

---

## 3. Test Tier Definitions

| Tier | Name | Focus & Purpose | Number of Tests |
|---|---|---|---|
| **Tier 1** | Feature Coverage | Verifies fundamental RPC query capabilities (Category filtering, Search, Pagination, Summaries, Source filtering). | 11 tests |
| **Tier 2** | Boundary & Corner Cases | Verifies system resiliency under boundary conditions (Empty results, Untrusted peers, Limit 0, Timeout expiry, Disconnect cleanup). | 6 tests |
| **Tier 3** | Pairwise Combinations | Verifies multi-parameter interactions (Category + Search + Paging, Source + Summary-only, Timeout + Connection drop). | 3 tests |
| **Tier 4** | Real-World Scenarios | Simulates end-user application flows (Opening "Images" folder, Searching "Downloads", Infinite scrolling, Reconnect & retry). | 4 tests |

---

## 4. Feature Inventory Mapping

| Feature ID | Feature Description | Tier 1 Tests | Tier 2/3/4 Coverage |
|---|---|---|---|
| **FEAT-01** | Category Remote Queries | `test_tier1_feature_query_images_category`, `test_tier1_feature_query_videos_category`, `test_tier1_feature_query_audio_category`, `test_tier1_feature_query_documents_category`, `test_tier1_feature_query_apks_category`, `test_tier1_feature_query_archives_category` | `test_tier3_pairwise_category_search_pagination`, `test_tier4_scenario_open_images_folder` |
| **FEAT-02** | Search Substring Filtering | `test_tier1_feature_query_search_substring` | `test_tier3_pairwise_category_search_pagination`, `test_tier4_scenario_open_downloads_search` |
| **FEAT-03** | Pagination (Offset & Limit) | `test_tier1_feature_pagination_offset_limit` | `test_tier2_boundary_zero_limit`, `test_tier4_scenario_multi_page_infinite_scroll` |
| **FEAT-04** | Category & Source Aggregation | `test_tier1_feature_summary_only_aggregation` | `test_tier3_pairwise_source_summary_only` |
| **FEAT-05** | Source Filtering | `test_tier1_feature_source_filtering_whatsapp`, `test_tier1_feature_source_filtering_camera` | `test_tier3_pairwise_source_summary_only` |
| **FEAT-06** | RPC Security & Peer Verification | `test_tier2_boundary_untrusted_peer_drop` | All tests (mutual trust setup) |
| **FEAT-07** | Timeout & Resource Cleanup | `test_tier2_boundary_timeout_expiry`, `test_tier2_boundary_disconnect_cleanup` | `test_tier3_pairwise_timeout_with_disconnect`, `test_tier4_scenario_device_reconnect_retry` |

---

## 5. Detailed Test Case Inventory

### Tier 1: Feature Coverage

1. **`test_tier1_feature_query_images_category`**
   - **Input**: Query target peer with `category: Some(RemoteFileCategory::Images)`, `summary_only: false`, `offset: 0`, `limit: 50`.
   - **Expected Output**: Returns 3 matching files, all with `RemoteFileCategory::Images`. `total_matching == 3`.
   - **Authoritative Source**: `PROJECT.md` § Interface Contracts, `protocol.rs` `RemoteFileCategory::Images`.

2. **`test_tier1_feature_query_videos_category`**
   - **Input**: Query target peer with `category: Some(RemoteFileCategory::Videos)`.
   - **Expected Output**: Returns files matching `RemoteFileCategory::Videos` category.

3. **`test_tier1_feature_query_audio_category`**
   - **Input**: Query target peer with `category: Some(RemoteFileCategory::Audio)`.
   - **Expected Output**: Returns files matching `RemoteFileCategory::Audio` category.

4. **`test_tier1_feature_query_documents_category`**
   - **Input**: Query target peer with `category: Some(RemoteFileCategory::Documents)`.
   - **Expected Output**: Returns files matching `RemoteFileCategory::Documents` category.

5. **`test_tier1_feature_query_apks_category`**
   - **Input**: Query target peer with `category: Some(RemoteFileCategory::Apks)`.
   - **Expected Output**: Returns files matching `RemoteFileCategory::Apks` category.

6. **`test_tier1_feature_query_archives_category`**
   - **Input**: Query target peer with `category: Some(RemoteFileCategory::Archives)`.
   - **Expected Output**: Returns files matching `RemoteFileCategory::Archives` category.

7. **`test_tier1_feature_query_search_substring`**
   - **Input**: Target peer has files `["vacation_photo.jpg", "document.pdf", "vacation_video.mp4"]`. Query `search_query: Some("vacation")`.
   - **Expected Output**: Returns 2 files (`vacation_photo.jpg` and `vacation_video.mp4`), `total_matching == 2`.

8. **`test_tier1_feature_pagination_offset_limit`**
   - **Input**: Target peer has 10 files (IDs 1..=10). Query `offset: 3`, `limit: 4`.
   - **Expected Output**: Returns exactly 4 files (IDs 4, 5, 6, 7), `total_matching == 10`.

9. **`test_tier1_feature_summary_only_aggregation`**
   - **Input**: Query `summary_only: true`.
   - **Expected Output**: `files` array is empty, `summary` is `Some(RemoteFilesSummary)` containing correct `type_counts` (e.g. `images: 120`, `videos: 15`) and `source_counts`.

10. **`test_tier1_feature_source_filtering_whatsapp`**
    - **Input**: Query `source: Some(RemoteFileSource::WhatsApp)`.
    - **Expected Output**: Returns only files from `RemoteFileSource::WhatsApp`.

11. **`test_tier1_feature_source_filtering_camera`**
    - **Input**: Query `source: Some(RemoteFileSource::Camera)`.
    - **Expected Output**: Returns only files from `RemoteFileSource::Camera`.

---

### Tier 2: Boundary & Corner Cases

12. **`test_tier2_boundary_empty_results`**
    - **Input**: Query category `Apks` when peer has 0 APK files.
    - **Expected Output**: `files` is empty `Vec`, `total_matching == 0`, `error == None`.

13. **`test_tier2_boundary_untrusted_peer_drop`**
    - **Input**: Untrusted peer sends `RemoteFilesQuery` without being in responder's `TrustStore`.
    - **Expected Output**: Responder drops request; requestor's `query_remote_files_sync()` times out gracefully without hanging or crashing.

14. **`test_tier2_boundary_zero_limit`**
    - **Input**: Query with `limit: 0`.
    - **Expected Output**: `files` array is empty, but `total_matching` reflects actual count of matching files on responder.

15. **`test_tier2_boundary_timeout_expiry`**
    - **Input**: Target peer ignores `RemoteFilesQueryReceived` (does not call `send_remote_files_response`). Query `timeout_secs: 1`.
    - **Expected Output**: Call fails with timeout error `"Remote files query timed out after 1s"`. Waiter entry is removed from engine state.

16. **`test_tier2_boundary_disconnect_cleanup`**
    - **Input**: Peer connection is severed/dropped immediately while query is pending.
    - **Expected Output**: `query_remote_files_sync()` fails with channel closed/disconnect error, and waiter is cleaned up without memory leak.

17. **`test_tier2_boundary_max_limit`**
    - **Input**: Query with large `limit` (1000).
    - **Expected Output**: Handles large limit cleanly without index out-of-bounds.

---

### Tier 3: Pairwise Combinations

18. **`test_tier3_pairwise_category_search_pagination`**
    - **Input**: Combine `category: Some(Images)`, `search_query: Some("beach")`, `offset: 2`, `limit: 3`.
    - **Expected Output**: Only images matching `"beach"` are included, sliced at offset 2 with max 3 entries.

19. **`test_tier3_pairwise_source_summary_only`**
    - **Input**: Combine `source: Some(Camera)` with `summary_only: true`.
    - **Expected Output**: Returns `summary` breakdown without returning file entries list.

20. **`test_tier3_pairwise_timeout_with_disconnect`**
    - **Input**: Short timeout (`timeout_secs: 2`) combined with abrupt TCP disconnect mid-query.
    - **Expected Output**: Disconnect error returns immediately before full 2-second timeout expires.

---

### Tier 4: Real-World Application Scenarios

21. **`test_tier4_scenario_open_images_folder`**
    - **Input**: Simulates user opening the "Images" tab in Remote Explorer view (macOS/Android/Windows).
    - **Expected Output**: Query returns `Images` category with full display metadata (`display_name`, `size_bytes`, `mime_type`, `content_uri`) within <500ms. Verifies primary requirement of `ORIGINAL_REQUEST.md`.

22. **`test_tier4_scenario_open_downloads_search`**
    - **Input**: Simulates user selecting "Downloads" source filter and typing `"invoice"` into search bar.
    - **Expected Output**: Correctly filtered subset of downloads matching `"invoice"`.

23. **`test_tier4_scenario_multi_page_infinite_scroll`**
    - **Input**: Simulates UI infinite scrolling over 100 remote files: Page 1 (`offset: 0, limit: 50`), Page 2 (`offset: 50, limit: 50`).
    - **Expected Output**: Page 1 returns files 0..50, Page 2 returns files 50..100, no duplicates across pages, total count matches 100.

24. **`test_tier4_scenario_device_reconnect_retry`**
    - **Input**: Peer drops offline during initial query. Requestor reconnects engine to peer and retries query.
    - **Expected Output**: First attempt fails with connection error; after reconnect, second attempt succeeds with complete result set.
