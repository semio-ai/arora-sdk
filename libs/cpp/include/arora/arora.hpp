#ifndef _ARORA_ARORA_HPP_
#define _ARORA_ARORA_HPP_

#include <cstdint>

extern "C" {
  // Call the function `method_id` of the module `module_id` with the
  // size-prefixed argument buffer `arg`; returns the size-prefixed result
  // buffer. A component guest (wasm32-wasip2) calls it over the
  // `arora:module/host` interface (src/arora/component.cpp); a core module
  // guest imports it from the engine.
#if defined(__wasip2__)
  std::uint8_t *arora_dispatch(
#else
  __attribute__((import_name("arora_dispatch"))) std::uint8_t *arora_dispatch(
#endif
    const std::uint8_t *const module_id,
    const std::uint8_t *const method_id,
    const std::uint8_t *const arg
  );
}

#endif
