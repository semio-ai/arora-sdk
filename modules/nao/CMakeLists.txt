cmake_minimum_required(VERSION 3.20)
project(nao CXX)

# Required cache vars (normally passed by the wrapping build.rs):
#   ARORA_MODULE_CLI            host bin (with arora-module-cpp sibling)
#   ARORA_BEHAVIOR_TREE_INCLUDE records dir
#   ARORA_BUFFERS_LIB           i686-musl static lib (.a)
#   ARORA_UTIL_LIB              i686-musl static lib (.a)
#   ARORA_CPP_SOURCE_DIR        source dir of libs/cpp
#   ARORA_INCLUDE_DIR           target/include (generated arora C headers)
# CMAKE_TOOLCHAIN_FILE must be the Mac Homebrew i686-musl toolchain.
foreach(VAR ARORA_MODULE_CLI ARORA_BEHAVIOR_TREE_INCLUDE ARORA_BUFFERS_LIB
            ARORA_UTIL_LIB ARORA_CPP_SOURCE_DIR ARORA_INCLUDE_DIR)
  if(NOT DEFINED ${VAR})
    message(FATAL_ERROR "Required cache variable ${VAR} not set.")
  endif()
endforeach()

set(CMAKE_CXX_STANDARD 14)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Code generation: module.yaml -> arora/{include,source}/nao.{hpp,cpp}
set(GENERATED_DIR ${CMAKE_CURRENT_BINARY_DIR}/arora)
add_custom_target(generate-nao ALL
  COMMAND ${ARORA_MODULE_CLI}
    --include ${ARORA_BEHAVIOR_TREE_INCLUDE}
    generate
    -l cpp
    -m ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
    -o ${GENERATED_DIR}
  BYPRODUCTS
    ${GENERATED_DIR}/include/nao.hpp
    ${GENERATED_DIR}/source/nao.cpp
    ${GENERATED_DIR}/module.yaml
)

include(FetchContent)
FetchContent_Declare(
  qi
  GIT_REPOSITORY https://github.com/semio-ai/libqi.git
  GIT_TAG 5663c31d674a32c5ec208864ca5acb802e425c40
)
FetchContent_MakeAvailable(qi)

# Imported targets pointing at the bindep static libs.
add_library(arora-buffers STATIC IMPORTED GLOBAL)
set_property(TARGET arora-buffers PROPERTY IMPORTED_LOCATION ${ARORA_BUFFERS_LIB})
set_property(TARGET arora-buffers PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_INCLUDE_DIR})

add_library(arora-util STATIC IMPORTED GLOBAL)
set_property(TARGET arora-util PROPERTY IMPORTED_LOCATION ${ARORA_UTIL_LIB})
set_property(TARGET arora-util PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_INCLUDE_DIR})

# arora-cpp from libs/cpp (the in-tree C++ helper around the C ABI).
add_subdirectory(${ARORA_CPP_SOURCE_DIR} arora-cpp-build)

add_library(nao SHARED
  src/nao.cpp
  ${GENERATED_DIR}/source/nao.cpp
)
add_dependencies(nao generate-nao)
target_include_directories(nao PRIVATE
  ${ARORA_INCLUDE_DIR}
  ${GENERATED_DIR}/include
)
target_link_libraries(nao PRIVATE
  qi
  ${CMAKE_DL_LIBS}
  arora-buffers
  arora-util
  arora-cpp
)
target_compile_options(nao PRIVATE -fPIC)

find_package(Threads)
if(Threads_FOUND)
  target_link_libraries(nao PRIVATE Threads::Threads ${CMAKE_THREAD_LIBS_INIT})
else()
  target_link_libraries(nao PRIVATE -pthread)
endif()
