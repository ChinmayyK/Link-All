//! Empirical challenge and stress test harness for `linkall_send_remote_files_response`.

use linkall_core::engine::{Engine, EngineConfig};
use linkall_core::ffi::{linkall_send_remote_files_response, linkall_stop, LinkAllHandle};
use linkall_core::protocol::{
    RemoteFileCategory, RemoteFileCategoryCounts, RemoteFileEntry, RemoteFileSource,
    RemoteFileSourceCounts, RemoteFilesSummary,
};
use std::ffi::CString;
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

fn create_test_handle() -> (*mut LinkAllHandle, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let config = EngineConfig {
        device_name: "ChallengerTestDevice".into(),
        port: 0,
        trust_store_path: temp_dir.path().join("trust.json"),
        peer_store_path: temp_dir.path().join("peers.json"),
        identity_path: temp_dir.path().join("identity.bin"),
        data_dir: temp_dir.path().join("data"),
        enable_discovery: false,
        ..EngineConfig::default()
    };
    let (event_tx, event_rx) = mpsc::channel(256);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let engine = rt.block_on(Engine::start(config, event_tx)).unwrap();

    let handle = Box::into_raw(Box::new(LinkAllHandleWrapper {
        engine,
        event_rx: std::sync::Mutex::new(Some(event_rx)),
    }));

    (handle as *mut LinkAllHandle, temp_dir)
}

#[repr(C)]
struct LinkAllHandleWrapper {
    engine: Engine,
    event_rx: std::sync::Mutex<Option<mpsc::Receiver<linkall_core::EngineEvent>>>,
}

#[test]
fn test_null_pointers() {
    let req = CString::new(Uuid::new_v4().to_string()).unwrap();
    let target = CString::new(Uuid::new_v4().to_string()).unwrap();

    unsafe {
        // 1. Null handle
        let res = linkall_send_remote_files_response(
            std::ptr::null_mut(),
            req.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            std::ptr::null(),
        );
        assert_eq!(res, 0, "Null handle should return 0");

        let (handle, _dir) = create_test_handle();
        assert!(!handle.is_null());

        // 2. Null request_id
        let res = linkall_send_remote_files_response(
            handle,
            std::ptr::null(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            std::ptr::null(),
        );
        assert_eq!(res, 0, "Null request_id should return 0");

        // 3. Null target_device_id
        let res = linkall_send_remote_files_response(
            handle,
            req.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            std::ptr::null(),
        );
        assert_eq!(res, 0, "Null target_device_id should return 0");

        // 4. Null optional parameters (summary_json, files_json, error_str)
        let res = linkall_send_remote_files_response(
            handle,
            req.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            10,
            std::ptr::null(),
        );
        assert_eq!(res, 1, "Null optional fields should succeed and return 1");

        linkall_stop(handle);
    }
}

#[test]
fn test_invalid_uuid_strings() {
    let (handle, _dir) = create_test_handle();
    let valid_uuid = CString::new(Uuid::new_v4().to_string()).unwrap();

    let invalid_uuids = [
        "",
        "not-a-uuid",
        "12345",
        "zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz",
        "12345678-1234-1234-1234-12345678901",   // too short
        "12345678-1234-1234-1234-1234567890123", // too long
    ];

    unsafe {
        for inv in invalid_uuids {
            let inv_cs = CString::new(inv).unwrap();

            // Invalid request_id
            let res1 = linkall_send_remote_files_response(
                handle,
                inv_cs.as_ptr(),
                valid_uuid.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
            );
            assert_eq!(res1, 0, "Invalid request_id ('{}') should return 0", inv);

            // Invalid target_device_id
            let res2 = linkall_send_remote_files_response(
                handle,
                valid_uuid.as_ptr(),
                inv_cs.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
            );
            assert_eq!(
                res2, 0,
                "Invalid target_device_id ('{}') should return 0",
                inv
            );
        }

        linkall_stop(handle);
    }
}

#[test]
fn test_empty_json_strings() {
    let (handle, _dir) = create_test_handle();
    let req = CString::new(Uuid::new_v4().to_string()).unwrap();
    let target = CString::new(Uuid::new_v4().to_string()).unwrap();

    let empty_str = CString::new("").unwrap();

    unsafe {
        let res = linkall_send_remote_files_response(
            handle,
            req.as_ptr(),
            target.as_ptr(),
            empty_str.as_ptr(),
            empty_str.as_ptr(),
            0,
            empty_str.as_ptr(),
        );
        assert_eq!(
            res, 1,
            "Empty JSON and error strings should be treated as None/empty and return 1"
        );

        linkall_stop(handle);
    }
}

#[test]
fn test_invalid_json_strings() {
    let (handle, _dir) = create_test_handle();
    let req = CString::new(Uuid::new_v4().to_string()).unwrap();
    let target = CString::new(Uuid::new_v4().to_string()).unwrap();

    let malformed_jsons = [
        "not json",
        "{invalid",
        "12345",
        "{\"foo\": \"bar\"}",
        "[1, 2, 3]",
    ];

    unsafe {
        for bad_json in malformed_jsons {
            let bad_cs = CString::new(bad_json).unwrap();

            let res = linkall_send_remote_files_response(
                handle,
                req.as_ptr(),
                target.as_ptr(),
                bad_cs.as_ptr(),
                bad_cs.as_ptr(),
                0,
                std::ptr::null(),
            );
            assert_eq!(
                res, 1,
                "Invalid JSON ('{}') should be handled safely and return 1",
                bad_json
            );
        }

        linkall_stop(handle);
    }
}

#[test]
fn test_non_empty_error_strings() {
    let (handle, _dir) = create_test_handle();
    let req = CString::new(Uuid::new_v4().to_string()).unwrap();
    let target = CString::new(Uuid::new_v4().to_string()).unwrap();

    let errors = [
        "Permission denied",
        "Storage quota exceeded",
        "Directory /sdcard/Photos does not exist",
    ];

    unsafe {
        for err_msg in errors {
            let err_cs = CString::new(err_msg).unwrap();

            let res = linkall_send_remote_files_response(
                handle,
                req.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                err_cs.as_ptr(),
            );
            assert_eq!(
                res, 1,
                "Non-empty error string ('{}') should return 1",
                err_msg
            );
        }

        linkall_stop(handle);
    }
}

#[test]
fn test_large_file_lists() {
    let (handle, _dir) = create_test_handle();
    let req = CString::new(Uuid::new_v4().to_string()).unwrap();
    let target = CString::new(Uuid::new_v4().to_string()).unwrap();

    // Create 5,000 file entries
    let mut entries = Vec::with_capacity(5000);
    for i in 0..5000 {
        entries.push(RemoteFileEntry {
            file_id: i,
            display_name: format!("file_{}.dat", i),
            size_bytes: 1024 * i,
            mime_type: "application/octet-stream".into(),
            date_modified: 1700000000 + i,
            category: RemoteFileCategory::Other,
            source: RemoteFileSource::Downloads,
            content_uri: format!("content://media/external/file/{}", i),
        });
    }

    let files_json = serde_json::to_string(&entries).unwrap();
    let files_cs = CString::new(files_json).unwrap();

    let summary = RemoteFilesSummary {
        type_counts: RemoteFileCategoryCounts {
            images: 0,
            videos: 0,
            audio: 0,
            documents: 0,
            apks: 0,
            archives: 0,
        },
        source_counts: RemoteFileSourceCounts {
            whatsapp: 0,
            downloads: 5000,
            camera: 0,
        },
    };
    let summary_json = serde_json::to_string(&summary).unwrap();
    let summary_cs = CString::new(summary_json).unwrap();

    unsafe {
        let res = linkall_send_remote_files_response(
            handle,
            req.as_ptr(),
            target.as_ptr(),
            summary_cs.as_ptr(),
            files_cs.as_ptr(),
            5000,
            std::ptr::null(),
        );
        assert_eq!(
            res, 1,
            "Large file list (5,000 items) should be handled cleanly and return 1"
        );

        linkall_stop(handle);
    }
}

#[test]
fn test_special_characters_in_json() {
    let (handle, _dir) = create_test_handle();
    let req = CString::new(Uuid::new_v4().to_string()).unwrap();
    let target = CString::new(Uuid::new_v4().to_string()).unwrap();

    let special_entries = vec![RemoteFileEntry {
        file_id: 42,
        display_name: "test_文件_😀_🚀_\"quoted\"_\n_newline.txt".into(),
        size_bytes: 4096,
        mime_type: "image/jpeg".into(),
        date_modified: 1700000000,
        category: RemoteFileCategory::Images,
        source: RemoteFileSource::Camera,
        content_uri: "content://media/external/file/42?path=/sdcard/DCIM/photos/😀_2026/test_文件_\"quoted\".jpg".into(),
    }];

    let files_json = serde_json::to_string(&special_entries).unwrap();
    let files_cs = CString::new(files_json).unwrap();

    let err_str_special = CString::new(
        "Failed to scan: path 'C:\\Users\\Test\\📁' containing spaces & \"quotes\"\nError detail: invalid char 💥"
    ).unwrap();

    unsafe {
        let res = linkall_send_remote_files_response(
            handle,
            req.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            files_cs.as_ptr(),
            1,
            err_str_special.as_ptr(),
        );
        assert_eq!(
            res, 1,
            "Special UTF-8 characters, emojis, quotes, and newlines should return 1"
        );

        linkall_stop(handle);
    }
}
