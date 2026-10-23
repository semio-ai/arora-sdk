#ifndef _ARORA_MODULE_HPP_
#define _ARORA_MODULE_HPP_

#include <cstdint>

extern "C" {
  // A function a module serves: its id, and its entry point, which takes the
  // size-prefixed argument buffer and returns the size-prefixed result buffer
  // (`arora_buffer_writer_finalize`'s).
  struct arora_module_export {
    const std::uint8_t *id;
    const std::uint8_t *(*function)(const std::uint8_t *arg);
  };

  // The functions the module serves, ended by an entry whose id is null.
  // arora-module-cli generates it with the module's sources; the component's
  // `functions` and `dispatch` exports (src/arora/component.cpp) serve it.
  extern const arora_module_export arora_module_exports[];
}

#endif
