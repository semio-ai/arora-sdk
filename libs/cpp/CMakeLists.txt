cmake_minimum_required(VERSION 3.17)
project(arora-cpp CXX)

# arora-cpp is a small C++ helper library that wraps the arora C ABI in a
# friendlier C++ interface. It is meant to be linked into wasm guest modules
# alongside the arora-buffers and arora-util static libraries.
#
# When invoked stand-alone, the caller must set ARORA_INCLUDE_DIR to a
# directory containing the generated arora C headers (produced by the
# arora-buffers and arora-util Rust crates during their build), and provide
# arora-buffers and arora-util as link targets (typically IMPORTED static
# libraries pointing at the wasm32-wasip2 outputs of those crates). When
# add_subdirectory'd from a module's CMakeLists.txt (e.g. modules/test-cpp),
# the parent CMake provides those targets.

if(NOT DEFINED ARORA_INCLUDE_DIR)
  message(FATAL_ERROR "ARORA_INCLUDE_DIR must be set so arora-cpp can find arora C headers.")
endif()

add_library(arora-cpp STATIC src/arora/buffer/deserialize.cpp)
target_include_directories(arora-cpp PUBLIC include ${ARORA_INCLUDE_DIR})
target_compile_options(arora-cpp PRIVATE -fPIC -O3 -flto)

# arora-buffers must be available as a target from the parent project. We do
# not link arora-c explicitly: it was an INTERFACE-only target in the old
# build, and the include paths it carried are now folded into ARORA_INCLUDE_DIR.
if(TARGET arora-buffers)
  target_link_libraries(arora-cpp PRIVATE arora-buffers)
endif()

# arora-cpp-component: what makes a guest a component of the `arora:module`
# world (wasm32-wasip2) — the world's `functions` and `dispatch` exports, and
# `arora_dispatch` over its `host` import (src/arora/component.cpp). Link it
# into the guest: an object library's objects are the guest's own, so exports
# nothing references are kept, and its link options name the world for
# wasm-component-ld, which the WASI SDK's wasm32-wasip2 toolchain links with.
# ARORA_WIT may name another copy of the world's WIT file.
set(ARORA_WIT ${CMAKE_CURRENT_LIST_DIR}/../../crates/arora-engine/wit/arora-module.wit
  CACHE FILEPATH "The arora:module world's WIT file, which C++ guests are components of")
add_library(arora-cpp-component OBJECT src/arora/component.cpp)
target_include_directories(arora-cpp-component PUBLIC include ${ARORA_INCLUDE_DIR})
target_compile_options(arora-cpp-component PRIVATE -O3 -flto)
# The canonical ABI allocates in the guest with its `cabi_realloc` export.
# wasi-libc and the Rust standard library of the staticlibs each define one,
# weak and not necessarily exported: export whichever the link keeps.
target_link_options(arora-cpp-component INTERFACE
  "LINKER:--component-type,${ARORA_WIT}"
  "LINKER:--export=cabi_realloc")

# arora_module_directory(<target> <directory> HEADER <header.json>
#                        [RECORDS <records dir>...])
#
# Add the target <target>-module-directory, which writes the module directory
# a device loads <target> from — the form a Rust module ships in too:
#
#   <directory>/header.json     the header arora-module-cli generated
#   <directory>/<target>.wasm   the guest
#   <directory>/records/        the records of every RECORDS directory, merged
#
# It depends on <target> and brings <directory> up to date on every build: the
# header and records change with module.yaml without the guest relinking, so
# the directory is not written only when the guest links. Build it, rather
# than <target>, to get the module directory. cmake/arora-module-directory.cmake
# does the writing.
function(arora_module_directory target directory)
  cmake_parse_arguments(ARG "" "HEADER" "RECORDS" ${ARGN})
  if(NOT ARG_HEADER)
    message(FATAL_ERROR "arora_module_directory(${target}): HEADER is required")
  endif()
  string(REPLACE ";" "|" records "${ARG_RECORDS}")
  add_custom_target(${target}-module-directory ALL
    COMMAND ${CMAKE_COMMAND}
      -DDIRECTORY=${directory}
      -DNAME=${target}
      -DGUEST=$<TARGET_FILE:${target}>
      -DHEADER=${ARG_HEADER}
      -DRECORDS=${records}
      -P ${CMAKE_CURRENT_FUNCTION_LIST_DIR}/cmake/arora-module-directory.cmake
    VERBATIM)
  add_dependencies(${target}-module-directory ${target})
  # The guest is a component of the world it links against: an edit to the
  # world relinks it.
  set_property(TARGET ${target} APPEND PROPERTY LINK_DEPENDS ${ARORA_WIT})
endfunction()
