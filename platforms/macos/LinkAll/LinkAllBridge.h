// LinkAllBridge.h
// C header for the Rust linkall-core FFI — add to Xcode bridging header.

#pragma once
#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// ── Opaque types ──────────────────────────────────────────────────────────────
typedef struct LinkAllHandle LinkAllHandle;
typedef struct PbEvent         PbEvent;

// ── Event type codes ──────────────────────────────────────────────────────────
#define PB_EVENT_NONE                    0
#define PB_EVENT_CLIPBOARD_TEXT          1   // auto-applied to local clipboard
#define PB_EVENT_CLIPBOARD_IMAGE         2   // auto-applied
#define PB_EVENT_CLIPBOARD_FILE          3   // auto-applied (legacy)
#define PB_EVENT_PAIRING_REQUESTED       4
#define PB_EVENT_PEER_CONNECTED          5
#define PB_EVENT_PEER_DISCONNECTED       6
#define PB_EVENT_WARNING                 7
#define PB_EVENT_CLIPBOARD_SYNCED        8
// 9, 10 reserved
#define PB_EVENT_CLIPBOARD_AVAILABLE    11   // timeline-first: in feed, NOT auto-applied
#define PB_EVENT_FILE_TRANSFER_INCOMING 12
#define PB_EVENT_FILE_TRANSFER_PROGRESS 13
#define PB_EVENT_FILE_TRANSFER_COMPLETE 14
#define PB_EVENT_FILE_TRANSFER_FAILED   15
#define PB_EVENT_ACTIVITY_UPDATED       16
#define PB_EVENT_SYSTEM_HEALTH_UPDATED  26

// ── Engine lifecycle ──────────────────────────────────────────────────────────
/// Start engine. Returns NULL on failure.
/// @param device_name UTF-8 device name, or NULL for auto-detection.
/// @param port        TCP port (0 = default 47823).
LinkAllHandle *linkall_start(const char *device_name, uint16_t port);

/// Stop and free the engine.
void linkall_stop(LinkAllHandle *handle);

// ── Clipboard push ────────────────────────────────────────────────────────────
int32_t linkall_push_text(LinkAllHandle *handle, const char *text);
int32_t linkall_push_image(LinkAllHandle *handle, const char *mime_type,
                             const uint8_t *data, size_t len);
int32_t linkall_push_file(LinkAllHandle *handle, const char *filename,
                            const uint8_t *data, size_t len);

// ── Event poll ────────────────────────────────────────────────────────────────
/// Non-blocking. Returns NULL if no event. Caller must free with linkall_free_event().
PbEvent *linkall_poll_event(LinkAllHandle *handle);
/// Returns the event type code for @p event.
int32_t  linkall_event_type(const PbEvent *event);
/// Free an event returned by linkall_poll_event().
void     linkall_free_event(PbEvent *event);

// ── Common event accessors ────────────────────────────────────────────────────
const char    *linkall_event_text(PbEvent *event);
const char    *linkall_event_device_name(PbEvent *event);
const char    *linkall_event_fingerprint(PbEvent *event);
const uint8_t *linkall_event_image_data(PbEvent *event, size_t *out_len,
                                          const char **out_mime);
const uint8_t *linkall_event_file_data(PbEvent *event, size_t *out_len,
                                         const char **out_name);

// ── Timeline-first clipboard ──────────────────────────────────────────────────
/// 1 if auto-applied; 0 if timeline-first (user must apply manually).
int32_t linkall_event_auto_applied(const PbEvent *event);
/// Activity feed entry ID; -1 if not applicable.
int64_t linkall_event_activity_id(const PbEvent *event);
/// Apply clipboard item to local clipboard by content hash. Returns 1 on success.
int32_t linkall_apply_clipboard(LinkAllHandle *handle, const char *hash);

// ── File transfer ─────────────────────────────────────────────────────────────
const char *linkall_event_transfer_id(PbEvent *event);
const char *linkall_event_transfer_file_name(PbEvent *event);
int32_t     linkall_event_transfer_percent(const PbEvent *event);
int64_t     linkall_event_transfer_total_bytes(const PbEvent *event);
int64_t     linkall_event_transfer_bytes_received(const PbEvent *event);
const char *linkall_event_transfer_dest_path(PbEvent *event);
int32_t     linkall_accept_file_transfer(LinkAllHandle *handle,
                                           const char *transfer_id_hex);
int32_t     linkall_reject_file_transfer(LinkAllHandle *handle,
                                           const char *transfer_id_hex);

// ── Remote Explorer (Phase 3) ─────────────────────────────────────────────────
#define PB_EVENT_REMOTE_FILES_QUERY        30
#define PB_EVENT_REMOTE_THUMBNAIL_REQUEST  31
#define PB_EVENT_REMOTE_FILE_PULL_REQUEST  32
#define PB_EVENT_REMOTE_FILES_RESPONSE     33
#define PB_EVENT_REMOTE_THUMBNAIL_RESPONSE 34

int32_t linkall_send_remote_files_query(LinkAllHandle *handle,
                                          const char *target_device_id,
                                          const char *request_id,
                                          int32_t summary_only,
                                          const char *category,
                                          const char *source,
                                          const char *search_query,
                                          uint32_t offset,
                                          uint32_t limit);
int32_t linkall_send_remote_thumbnail_request(LinkAllHandle *handle,
                                                const char *target_device_id,
                                                const char *request_id,
                                                uint64_t file_id,
                                                uint32_t size_px);
int32_t linkall_send_remote_file_pull_request(LinkAllHandle *handle,
                                                const char *target_device_id,
                                                const char *request_id,
                                                uint64_t file_id);
int32_t linkall_send_remote_files_response(LinkAllHandle *handle,
                                             const char *request_id,
                                             const char *target_device_id,
                                             const char *summary_json,
                                             const char *files_json,
                                             uint32_t total_matching,
                                             const char *error_str);

const char *linkall_event_remote_request_id(PbEvent *event);
const char *linkall_event_remote_summary_json(PbEvent *event);
const char *linkall_event_remote_files_json(PbEvent *event);
uint32_t    linkall_event_remote_total_matching(const PbEvent *event);
uint64_t    linkall_event_remote_file_id(const PbEvent *event);
const uint8_t *linkall_event_remote_thumbnail_data(PbEvent *event);
size_t      linkall_event_remote_thumbnail_len(const PbEvent *event);
const char *linkall_event_remote_error(PbEvent *event);

#ifdef __cplusplus
}
#endif
