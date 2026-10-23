// The `arora:module` world (crates/arora-engine/wit/arora-module.wit) for a
// C++ guest built as a component (wasm32-wasip2): its exports `functions` and
// `dispatch`, serving the module's `arora_module_exports`, and
// `arora_dispatch`, which generated code calls another module through, over
// its `host.dispatch` import. Each is the core function `wasm-component-ld`
// lifts or lowers, in the canonical ABI's form; the guest links with
// `--component-type` naming the world, which arora-cpp-component's link
// options carry.
//
// An Arora Buffer starts with its length, as a little-endian u32 that counts
// those four bytes too; it crosses the boundary as a `list<u8>` of that
// length.

#if defined(__wasip2__)

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>

#include <arora/module.hpp>

extern "C" {
#include <arora/buffers.h>
}

namespace {

// Where an export's results go: a `list<id>` (a pointer and a length), or a
// `result<list<u8>, string>` (its case, then the list's pointer and length at
// offsets 4 and 8).
alignas(8) std::uint8_t results[12];

std::uint32_t load_u32(const std::uint8_t *at) {
  std::uint32_t value;
  std::memcpy(&value, at, sizeof value);
  return value;
}

void store_u32(std::uint8_t *at, std::uint32_t value) { std::memcpy(at, &value, sizeof value); }

void store_u64(std::uint8_t *at, std::uint64_t value) { std::memcpy(at, &value, sizeof value); }

// An id's halves, as the `id` record holds them: the UUID's bytes, read big-endian.
std::uint64_t high_half(const std::uint8_t *id) {
  std::uint64_t half = 0;
  for (int i = 0; i < 8; ++i) half = (half << 8) | id[i];
  return half;
}

std::uint64_t low_half(const std::uint8_t *id) { return high_half(id + 8); }

// Free a list the canonical ABI allocated with `cabi_realloc`: an empty one
// was not allocated (its pointer is its alignment).
void free_list(void *list, std::size_t length) {
  if (length != 0) std::free(list);
}

void store_id(std::uint8_t *at, std::uint64_t high, std::uint64_t low) {
  for (int i = 7; i >= 0; --i, high >>= 8) at[i] = static_cast<std::uint8_t>(high);
  for (int i = 15; i >= 8; --i, low >>= 8) at[i] = static_cast<std::uint8_t>(low);
}

// The result `result<list<u8>, string>`: `ok` with `ok_list`, or `err` with a
// copy of `message`.
std::uint8_t *store_result(const std::uint8_t *ok_list, const char *message) {
  if (ok_list != nullptr) {
    results[0] = 0;
    store_u32(results + 4, static_cast<std::uint32_t>(reinterpret_cast<std::uintptr_t>(ok_list)));
    store_u32(results + 8, load_u32(ok_list));
    return results;
  }
  std::size_t length = std::strlen(message);
  auto *copy = static_cast<char *>(std::malloc(length == 0 ? 1 : length));
  std::memcpy(copy, message, length);
  results[0] = 1;
  store_u32(results + 4, static_cast<std::uint32_t>(reinterpret_cast<std::uintptr_t>(copy)));
  store_u32(results + 8, static_cast<std::uint32_t>(length));
  return results;
}

}  // namespace

extern "C" {

// `functions: func() -> list<id>`.
__attribute__((export_name("functions"))) std::uint8_t *arora_component_functions() {
  std::size_t count = 0;
  while (arora_module_exports[count].id != nullptr) ++count;
  // An id is two u64 halves: 16 bytes, aligned to 8, which malloc's are.
  auto *ids = static_cast<std::uint8_t *>(std::malloc(count == 0 ? 1 : count * 16));
  for (std::size_t i = 0; i < count; ++i) {
    store_u64(ids + i * 16, high_half(arora_module_exports[i].id));
    store_u64(ids + i * 16 + 8, low_half(arora_module_exports[i].id));
  }
  store_u32(results, static_cast<std::uint32_t>(reinterpret_cast<std::uintptr_t>(ids)));
  store_u32(results + 4, static_cast<std::uint32_t>(count));
  return results;
}

__attribute__((export_name("cabi_post_functions"))) void arora_component_post_functions(std::uint8_t *returned) {
  std::free(reinterpret_cast<void *>(static_cast<std::uintptr_t>(load_u32(returned))));
}

// `dispatch: func(method: id, arg: list<u8>) -> result<list<u8>, string>`.
// The guest owns `arg`, allocated with its `cabi_realloc`. A generated entry
// point reads the argument as the Arora Buffer it starts with, so one that is
// not one is refused (`arora_buffer_well_formed`).
__attribute__((export_name("dispatch"))) std::uint8_t *arora_component_dispatch(
    std::int64_t method_high, std::int64_t method_low, std::uint8_t *arg, std::size_t arg_length) {
  std::uint8_t method[16];
  store_id(method, static_cast<std::uint64_t>(method_high), static_cast<std::uint64_t>(method_low));
  for (const arora_module_export *served = arora_module_exports; served->id != nullptr; ++served) {
    if (std::memcmp(served->id, method, sizeof method) == 0) {
      if (!arora_buffer_well_formed(arg, arg_length)) {
        free_list(arg, arg_length);
        return store_result(nullptr, "the argument is not an Arora Buffer");
      }
      const std::uint8_t *result = served->function(arg);
      free_list(arg, arg_length);
      return store_result(result, nullptr);
    }
  }
  free_list(arg, arg_length);
  return store_result(nullptr, "the module serves no function with this id");
}

__attribute__((export_name("cabi_post_dispatch"))) void arora_component_post_dispatch(std::uint8_t *returned) {
  auto *list = reinterpret_cast<std::uint8_t *>(static_cast<std::uintptr_t>(load_u32(returned + 4)));
  if (returned[0] == 0) {
    arora_buffer_writer_finalized_free(list);
  } else {
    std::free(list);
  }
}

// `host.dispatch: func(module: id, method: id, arg: list<u8>) -> result<list<u8>, string>`.
__attribute__((import_module("arora:module/host@0.2.0"), import_name("dispatch"))) void arora_host_dispatch(
    std::int64_t module_high, std::int64_t module_low, std::int64_t method_high, std::int64_t method_low,
    const std::uint8_t *arg, std::size_t arg_length, std::uint8_t *returned);

// The callee's result buffer; an error the host returns comes back as an error
// buffer, as does a result that is not an Arora Buffer, which generated code
// would misread (`arora_buffer_well_formed`).
//
// The buffer stays valid until the next call, which frees it, so a guest holds
// one result however many calls it makes. Generated code reads a result before
// it calls again: on a core module, the engine writes each result into a
// buffer the guest's arora-buffers pool hands out again.
std::uint8_t *arora_dispatch(const std::uint8_t *const module_id, const std::uint8_t *const method_id,
                             const std::uint8_t *const arg) {
  // The latest result, and whether `arora_buffer_writer_finalize` made it.
  static std::uint8_t *latest = nullptr;
  static bool latest_finalized = false;
  auto keep = [](std::uint8_t *result, bool finalized) {
    if (latest_finalized) {
      arora_buffer_writer_finalized_free(latest);
    } else {
      std::free(latest);
    }
    latest = result;
    latest_finalized = finalized;
    return result;
  };

  alignas(4) std::uint8_t returned[12];
  arora_host_dispatch(static_cast<std::int64_t>(high_half(module_id)), static_cast<std::int64_t>(low_half(module_id)),
                      static_cast<std::int64_t>(high_half(method_id)), static_cast<std::int64_t>(low_half(method_id)),
                      arg, load_u32(arg), returned);
  auto *list = reinterpret_cast<std::uint8_t *>(static_cast<std::uintptr_t>(load_u32(returned + 4)));
  std::uint32_t length = load_u32(returned + 8);
  if (returned[0] == 0 && arora_buffer_well_formed(list, length)) return keep(list, false);
  arora_buffer_writer *writer = arora_buffer_writer_new();
  if (returned[0] == 0) {
    static const char not_a_buffer[] = "the result is not an Arora Buffer";
    arora_buffer_writer_add_error(writer, reinterpret_cast<const std::uint8_t *>(not_a_buffer),
                                  sizeof not_a_buffer - 1);
  } else {
    arora_buffer_writer_add_error(writer, list, length);
  }
  free_list(list, length);
  std::uint8_t *error = arora_buffer_writer_finalize(writer, nullptr);
  arora_buffer_writer_free(writer);
  return keep(error, true);
}

}  // extern "C"

#endif
