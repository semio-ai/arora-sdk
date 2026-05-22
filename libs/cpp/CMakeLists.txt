cmake_minimum_required(VERSION 3.14)
project(arora-cpp CXX)

# arora-cpp is a small C++ helper library that wraps the arora C ABI in a
# friendlier C++ interface. It is meant to be linked into wasm guest modules
# alongside the arora-buffers and arora-util static libraries.
#
# When invoked stand-alone, the caller must set ARORA_INCLUDE_DIR to a
# directory containing the generated arora C headers (produced by the
# arora-buffers and arora-util Rust crates during their build), and provide
# arora-buffers and arora-util as link targets (typically IMPORTED static
# libraries pointing at the wasm32-wasip1 outputs of those crates). When
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
