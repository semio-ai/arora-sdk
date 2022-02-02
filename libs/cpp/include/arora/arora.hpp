#ifndef _ARORA_ARORA_HPP_
#define _ARORA_ARORA_HPP_

#include <cstdint>

extern "C" {
  __attribute__((import_name("arora_dispatch"))) std::uint8_t *arora_dispatch(
    const std::uint8_t *const module_id,
    const std::uint8_t *const method_id,
    const std::uint8_t *const arg
  );
}

#endif