#ifndef VLDB_CONTROLLER_FFI_H
#define VLDB_CONTROLLER_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define VLDB_CONTROLLER_FFI_PROCESS_MODE_SERVICE 0
#define VLDB_CONTROLLER_FFI_PROCESS_MODE_MANAGED 1

#define VLDB_CONTROLLER_FFI_SPACE_KIND_ROOT 0
#define VLDB_CONTROLLER_FFI_SPACE_KIND_USER 1
#define VLDB_CONTROLLER_FFI_SPACE_KIND_PROJECT 2

#define VLDB_CONTROLLER_FFI_SQLITE_VALUE_INT64 0
#define VLDB_CONTROLLER_FFI_SQLITE_VALUE_FLOAT64 1
#define VLDB_CONTROLLER_FFI_SQLITE_VALUE_STRING 2
#define VLDB_CONTROLLER_FFI_SQLITE_VALUE_BYTES 3
#define VLDB_CONTROLLER_FFI_SQLITE_VALUE_BOOL 4
#define VLDB_CONTROLLER_FFI_SQLITE_VALUE_NULL 5

#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_UNSPECIFIED 0
#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_STRING 1
#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_INT64 2
#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_FLOAT64 3
#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_BOOL 4
#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_VECTOR_FLOAT32 5
#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_FLOAT32 6
#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_UINT64 7
#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_INT32 8
#define VLDB_CONTROLLER_FFI_LANCEDB_COLUMN_UINT32 9

#define VLDB_CONTROLLER_FFI_LANCEDB_INPUT_UNSPECIFIED 0
#define VLDB_CONTROLLER_FFI_LANCEDB_INPUT_JSON_ROWS 1
#define VLDB_CONTROLLER_FFI_LANCEDB_INPUT_ARROW_IPC 2

#define VLDB_CONTROLLER_FFI_LANCEDB_OUTPUT_UNSPECIFIED 0
#define VLDB_CONTROLLER_FFI_LANCEDB_OUTPUT_ARROW_IPC 1
#define VLDB_CONTROLLER_FFI_LANCEDB_OUTPUT_JSON_ROWS 2

typedef struct FfiControllerClientHandle FfiControllerClientHandle;

typedef struct FfiControllerClientConfig {
    const char* endpoint;
    uint8_t auto_spawn;
    const char* spawn_executable;
    int32_t spawn_process_mode;
    uint64_t minimum_uptime_secs;
    uint64_t idle_timeout_secs;
    uint64_t default_lease_ttl_secs;
    uint64_t connect_timeout_secs;
    uint64_t startup_timeout_secs;
    uint64_t startup_retry_interval_ms;
    uint64_t lease_renew_interval_secs;
} FfiControllerClientConfig;

typedef struct FfiClientRegistration {
    const char* client_id;
    const char* host_kind;
    uint32_t process_id;
    const char* process_name;
    uint64_t lease_ttl_secs;
} FfiClientRegistration;

typedef struct FfiSpaceRegistration {
    const char* space_id;
    const char* space_label;
    int32_t space_kind;
    const char* space_root;
} FfiSpaceRegistration;

typedef struct FfiControllerSqliteEnableRequest {
    const char* space_id;
    const char* db_path;
    uint64_t connection_pool_size;
    uint64_t busy_timeout_ms;
    const char* journal_mode;
    const char* synchronous;
    uint8_t foreign_keys;
    const char* temp_store;
    uint32_t wal_autocheckpoint_pages;
    int64_t cache_size_kib;
    uint64_t mmap_size_bytes;
    uint8_t enforce_db_file_lock;
    uint8_t read_only;
    uint8_t allow_uri_filenames;
    uint8_t trusted_schema;
    uint8_t defensive;
} FfiControllerSqliteEnableRequest;

typedef struct FfiControllerLanceDbEnableRequest {
    const char* space_id;
    const char* default_db_path;
    const char* db_root;
    uint64_t read_consistency_interval_ms;
    uint64_t max_upsert_payload;
    uint64_t max_search_limit;
    uint64_t max_concurrent_requests;
} FfiControllerLanceDbEnableRequest;

typedef struct FfiSpaceBackendStatus {
    uint8_t enabled;
    char* mode;
    char* target;
} FfiSpaceBackendStatus;

typedef struct FfiSpaceSnapshot {
    char* space_id;
    char* space_label;
    int32_t space_kind;
    char* space_root;
    uint64_t attached_clients;
    FfiSpaceBackendStatus* sqlite;
    FfiSpaceBackendStatus* lancedb;
} FfiSpaceSnapshot;

typedef struct FfiSpaceSnapshotArray {
    FfiSpaceSnapshot* items;
    size_t len;
} FfiSpaceSnapshotArray;

typedef struct FfiControllerStatusSnapshot {
    int32_t process_mode;
    char* bind_addr;
    uint64_t started_at_unix_ms;
    uint64_t last_request_at_unix_ms;
    uint64_t minimum_uptime_secs;
    uint64_t idle_timeout_secs;
    uint64_t default_lease_ttl_secs;
    uint64_t active_clients;
    uint64_t attached_spaces;
    uint64_t inflight_requests;
    uint8_t shutdown_candidate;
} FfiControllerStatusSnapshot;

typedef struct FfiByteBuffer {
    uint8_t* data;
    size_t len;
} FfiByteBuffer;

typedef struct FfiByteBufferArray {
    FfiByteBuffer* items;
    size_t len;
} FfiByteBufferArray;

typedef struct FfiStringArray {
    const char* const* items;
    size_t len;
} FfiStringArray;

typedef struct FfiSqliteValue {
    int32_t kind;
    int64_t int64_value;
    double float64_value;
    const char* string_value;
    const uint8_t* bytes_value;
    size_t bytes_len;
    uint8_t bool_value;
} FfiSqliteValue;

typedef struct FfiSqliteBatchItem {
    const FfiSqliteValue* params;
    size_t params_len;
} FfiSqliteBatchItem;

typedef struct FfiControllerSqliteExecuteResult {
    uint8_t success;
    char* message;
    int64_t rows_changed;
    int64_t last_insert_rowid;
} FfiControllerSqliteExecuteResult;

typedef struct FfiControllerSqliteExecuteBatchResult {
    uint8_t success;
    char* message;
    int64_t rows_changed;
    int64_t last_insert_rowid;
    int64_t statements_executed;
} FfiControllerSqliteExecuteBatchResult;

typedef struct FfiControllerSqliteQueryResult {
    char* json_data;
    uint64_t row_count;
} FfiControllerSqliteQueryResult;

typedef struct FfiControllerSqliteQueryStreamResult {
    FfiByteBufferArray* chunks;
    uint64_t row_count;
    uint64_t chunk_count;
    uint64_t total_bytes;
} FfiControllerSqliteQueryStreamResult;

typedef struct FfiControllerSqliteTokenizeResult {
    char* tokenizer_mode;
    char* normalized_text;
    char* tokens_json;
    char* fts_query;
} FfiControllerSqliteTokenizeResult;

typedef struct FfiControllerSqliteCustomWordEntry {
    char* word;
    uint64_t weight;
} FfiControllerSqliteCustomWordEntry;

typedef struct FfiControllerSqliteCustomWordArray {
    FfiControllerSqliteCustomWordEntry* items;
    size_t len;
} FfiControllerSqliteCustomWordArray;

typedef struct FfiControllerSqliteDictionaryMutationResult {
    uint8_t success;
    char* message;
    uint64_t affected_rows;
} FfiControllerSqliteDictionaryMutationResult;

typedef struct FfiControllerSqliteListCustomWordsResult {
    uint8_t success;
    char* message;
    FfiControllerSqliteCustomWordArray* words;
} FfiControllerSqliteListCustomWordsResult;

typedef struct FfiControllerSqliteEnsureFtsIndexResult {
    uint8_t success;
    char* message;
    char* index_name;
    char* tokenizer_mode;
} FfiControllerSqliteEnsureFtsIndexResult;

typedef struct FfiControllerSqliteRebuildFtsIndexResult {
    uint8_t success;
    char* message;
    char* index_name;
    char* tokenizer_mode;
    uint64_t reindexed_rows;
} FfiControllerSqliteRebuildFtsIndexResult;

typedef struct FfiControllerSqliteFtsMutationResult {
    uint8_t success;
    char* message;
    uint64_t affected_rows;
    char* index_name;
} FfiControllerSqliteFtsMutationResult;

typedef struct FfiControllerSqliteSearchFtsHit {
    char* id;
    char* file_path;
    char* title;
    char* title_highlight;
    char* content_snippet;
    double score;
    uint64_t rank;
    double raw_score;
} FfiControllerSqliteSearchFtsHit;

typedef struct FfiControllerSqliteSearchFtsHitArray {
    FfiControllerSqliteSearchFtsHit* items;
    size_t len;
} FfiControllerSqliteSearchFtsHitArray;

typedef struct FfiControllerSqliteSearchFtsResult {
    uint8_t success;
    char* message;
    char* index_name;
    char* tokenizer_mode;
    char* normalized_query;
    char* fts_query;
    char* source;
    char* query_mode;
    uint64_t total;
    FfiControllerSqliteSearchFtsHitArray* hits;
} FfiControllerSqliteSearchFtsResult;

typedef struct FfiControllerLanceDbCreateTableResult {
    char* message;
} FfiControllerLanceDbCreateTableResult;

typedef struct FfiControllerLanceDbUpsertResult {
    char* message;
    uint64_t version;
    uint64_t input_rows;
    uint64_t inserted_rows;
    uint64_t updated_rows;
    uint64_t deleted_rows;
} FfiControllerLanceDbUpsertResult;

typedef struct FfiControllerLanceDbSearchResult {
    char* message;
    char* format;
    uint64_t rows;
    uint8_t* data;
    size_t data_len;
} FfiControllerLanceDbSearchResult;

typedef struct FfiControllerLanceDbDeleteResult {
    char* message;
    uint64_t version;
    uint64_t deleted_rows;
} FfiControllerLanceDbDeleteResult;

typedef struct FfiControllerLanceDbDropTableResult {
    char* message;
} FfiControllerLanceDbDropTableResult;

typedef struct FfiControllerLanceDbColumnDef {
    const char* name;
    int32_t column_type;
    uint32_t vector_dim;
    uint8_t nullable;
} FfiControllerLanceDbColumnDef;

char* vldb_controller_ffi_version(void);
void vldb_controller_ffi_string_free(char* value);
void vldb_controller_ffi_bytes_free(uint8_t* data, size_t len);
void vldb_controller_ffi_byte_buffer_array_free(FfiByteBufferArray* value);

int vldb_controller_ffi_client_create(const FfiControllerClientConfig* config, const FfiClientRegistration* registration, FfiControllerClientHandle** client_out, char** error_out);
int vldb_controller_ffi_client_create_json(const char* request_json, FfiControllerClientHandle** client_out, char** response_out, char** error_out);
void vldb_controller_ffi_client_free(FfiControllerClientHandle* client);

int vldb_controller_ffi_client_connect(FfiControllerClientHandle* client, char** error_out);
int vldb_controller_ffi_client_connect_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
int vldb_controller_ffi_client_shutdown(FfiControllerClientHandle* client, char** error_out);
int vldb_controller_ffi_client_shutdown_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);

int vldb_controller_ffi_client_get_status(FfiControllerClientHandle* client, FfiControllerStatusSnapshot** status_out, char** error_out);
int vldb_controller_ffi_client_get_status_json(FfiControllerClientHandle* client, char** response_out, char** error_out);
void vldb_controller_ffi_controller_status_free(FfiControllerStatusSnapshot* value);

int vldb_controller_ffi_client_attach_space(FfiControllerClientHandle* client, const FfiSpaceRegistration* registration, FfiSpaceSnapshot** space_out, char** error_out);
int vldb_controller_ffi_client_attach_space_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
int vldb_controller_ffi_client_detach_space(FfiControllerClientHandle* client, const char* space_id, uint8_t* detached_out, char** error_out);
int vldb_controller_ffi_client_detach_space_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
int vldb_controller_ffi_client_list_spaces(FfiControllerClientHandle* client, FfiSpaceSnapshotArray** spaces_out, char** error_out);
int vldb_controller_ffi_client_list_spaces_json(FfiControllerClientHandle* client, char** response_out, char** error_out);
void vldb_controller_ffi_space_snapshot_free(FfiSpaceSnapshot* value);
void vldb_controller_ffi_space_snapshot_array_free(FfiSpaceSnapshotArray* value);

int vldb_controller_ffi_client_enable_sqlite(FfiControllerClientHandle* client, const FfiControllerSqliteEnableRequest* request, char** error_out);
int vldb_controller_ffi_client_enable_sqlite_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
int vldb_controller_ffi_client_disable_sqlite(FfiControllerClientHandle* client, const char* space_id, uint8_t* disabled_out, char** error_out);
int vldb_controller_ffi_client_disable_sqlite_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
int vldb_controller_ffi_client_execute_sqlite_script(FfiControllerClientHandle* client, const char* space_id, const char* sql, const FfiSqliteValue* params, size_t params_len, FfiControllerSqliteExecuteResult** result_out, char** error_out);
int vldb_controller_ffi_client_execute_sqlite_script_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_execute_result_free(FfiControllerSqliteExecuteResult* value);
int vldb_controller_ffi_client_execute_sqlite_batch(FfiControllerClientHandle* client, const char* space_id, const char* sql, const FfiSqliteBatchItem* items, size_t items_len, FfiControllerSqliteExecuteBatchResult** result_out, char** error_out);
int vldb_controller_ffi_client_execute_sqlite_batch_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_execute_batch_result_free(FfiControllerSqliteExecuteBatchResult* value);
int vldb_controller_ffi_client_query_sqlite_json(FfiControllerClientHandle* client, const char* space_id, const char* sql, const FfiSqliteValue* params, size_t params_len, FfiControllerSqliteQueryResult** result_out, char** error_out);
int vldb_controller_ffi_client_query_sqlite_json_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_query_result_free(FfiControllerSqliteQueryResult* value);
int vldb_controller_ffi_client_query_sqlite_stream(FfiControllerClientHandle* client, const char* space_id, const char* sql, const FfiSqliteValue* params, size_t params_len, uint64_t target_chunk_size, FfiControllerSqliteQueryStreamResult** result_out, char** error_out);
int vldb_controller_ffi_client_query_sqlite_stream_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_query_stream_result_free(FfiControllerSqliteQueryStreamResult* value);
int vldb_controller_ffi_client_tokenize_sqlite_text(FfiControllerClientHandle* client, const char* space_id, const char* tokenizer_mode, const char* text, uint8_t search_mode, FfiControllerSqliteTokenizeResult** result_out, char** error_out);
int vldb_controller_ffi_client_tokenize_sqlite_text_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_tokenize_result_free(FfiControllerSqliteTokenizeResult* value);
int vldb_controller_ffi_client_list_sqlite_custom_words(FfiControllerClientHandle* client, const char* space_id, FfiControllerSqliteListCustomWordsResult** result_out, char** error_out);
int vldb_controller_ffi_client_list_sqlite_custom_words_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_custom_word_array_free(FfiControllerSqliteCustomWordArray* value);
void vldb_controller_ffi_sqlite_list_custom_words_result_free(FfiControllerSqliteListCustomWordsResult* value);
int vldb_controller_ffi_client_upsert_sqlite_custom_word(FfiControllerClientHandle* client, const char* space_id, const char* word, uint32_t weight, FfiControllerSqliteDictionaryMutationResult** result_out, char** error_out);
int vldb_controller_ffi_client_upsert_sqlite_custom_word_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
int vldb_controller_ffi_client_remove_sqlite_custom_word(FfiControllerClientHandle* client, const char* space_id, const char* word, FfiControllerSqliteDictionaryMutationResult** result_out, char** error_out);
int vldb_controller_ffi_client_remove_sqlite_custom_word_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_dictionary_mutation_result_free(FfiControllerSqliteDictionaryMutationResult* value);
int vldb_controller_ffi_client_ensure_sqlite_fts_index(FfiControllerClientHandle* client, const char* space_id, const char* index_name, const char* tokenizer_mode, FfiControllerSqliteEnsureFtsIndexResult** result_out, char** error_out);
int vldb_controller_ffi_client_ensure_sqlite_fts_index_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_ensure_fts_index_result_free(FfiControllerSqliteEnsureFtsIndexResult* value);
int vldb_controller_ffi_client_rebuild_sqlite_fts_index(FfiControllerClientHandle* client, const char* space_id, const char* index_name, const char* tokenizer_mode, FfiControllerSqliteRebuildFtsIndexResult** result_out, char** error_out);
int vldb_controller_ffi_client_rebuild_sqlite_fts_index_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_rebuild_fts_index_result_free(FfiControllerSqliteRebuildFtsIndexResult* value);
int vldb_controller_ffi_client_upsert_sqlite_fts_document(FfiControllerClientHandle* client, const char* space_id, const char* index_name, const char* tokenizer_mode, const char* id, const char* file_path, const char* title, const char* content, FfiControllerSqliteFtsMutationResult** result_out, char** error_out);
int vldb_controller_ffi_client_upsert_sqlite_fts_document_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
int vldb_controller_ffi_client_delete_sqlite_fts_document(FfiControllerClientHandle* client, const char* space_id, const char* index_name, const char* id, FfiControllerSqliteFtsMutationResult** result_out, char** error_out);
int vldb_controller_ffi_client_delete_sqlite_fts_document_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_fts_mutation_result_free(FfiControllerSqliteFtsMutationResult* value);
int vldb_controller_ffi_client_search_sqlite_fts(FfiControllerClientHandle* client, const char* space_id, const char* index_name, const char* tokenizer_mode, const char* query, uint32_t limit, uint32_t offset, FfiControllerSqliteSearchFtsResult** result_out, char** error_out);
int vldb_controller_ffi_client_search_sqlite_fts_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_sqlite_search_fts_hit_array_free(FfiControllerSqliteSearchFtsHitArray* value);
void vldb_controller_ffi_sqlite_search_fts_result_free(FfiControllerSqliteSearchFtsResult* value);

int vldb_controller_ffi_client_enable_lancedb(FfiControllerClientHandle* client, const FfiControllerLanceDbEnableRequest* request, char** error_out);
int vldb_controller_ffi_client_enable_lancedb_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
int vldb_controller_ffi_client_disable_lancedb(FfiControllerClientHandle* client, const char* space_id, uint8_t* disabled_out, char** error_out);
int vldb_controller_ffi_client_disable_lancedb_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
int vldb_controller_ffi_client_create_lancedb_table(FfiControllerClientHandle* client, const char* space_id, const char* table_name, const FfiControllerLanceDbColumnDef* columns, size_t columns_len, uint8_t overwrite_if_exists, FfiControllerLanceDbCreateTableResult** result_out, char** error_out);
int vldb_controller_ffi_client_create_lancedb_table_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_lancedb_create_table_result_free(FfiControllerLanceDbCreateTableResult* value);
int vldb_controller_ffi_client_upsert_lancedb(FfiControllerClientHandle* client, const char* space_id, const char* table_name, int32_t input_format, const uint8_t* data, size_t data_len, const char* const* key_columns, size_t key_columns_len, FfiControllerLanceDbUpsertResult** result_out, char** error_out);
int vldb_controller_ffi_client_upsert_lancedb_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_lancedb_upsert_result_free(FfiControllerLanceDbUpsertResult* value);
int vldb_controller_ffi_client_search_lancedb(FfiControllerClientHandle* client, const char* space_id, const char* table_name, const float* vector, size_t vector_len, uint32_t limit, const char* filter, const char* vector_column, int32_t output_format, FfiControllerLanceDbSearchResult** result_out, char** error_out);
int vldb_controller_ffi_client_search_lancedb_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_lancedb_search_result_free(FfiControllerLanceDbSearchResult* value);
int vldb_controller_ffi_client_delete_lancedb(FfiControllerClientHandle* client, const char* space_id, const char* table_name, const char* condition, FfiControllerLanceDbDeleteResult** result_out, char** error_out);
int vldb_controller_ffi_client_delete_lancedb_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_lancedb_delete_result_free(FfiControllerLanceDbDeleteResult* value);
int vldb_controller_ffi_client_drop_lancedb_table(FfiControllerClientHandle* client, const char* space_id, const char* table_name, FfiControllerLanceDbDropTableResult** result_out, char** error_out);
int vldb_controller_ffi_client_drop_lancedb_table_json(FfiControllerClientHandle* client, const char* request_json, char** response_out, char** error_out);
void vldb_controller_ffi_lancedb_drop_table_result_free(FfiControllerLanceDbDropTableResult* value);

#ifdef __cplusplus
}
#endif

#endif
