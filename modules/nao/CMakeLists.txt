cmake_minimum_required(VERSION 3.20)
project(nao)

set(CMAKE_CXX_STANDARD 14)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -std=c++14")

message(STATUS "Generating C++ with:" ${ARORA_MODULE_CLI}
--include ${ARORA_BEHAVIOR_TREE_INCLUDE}
generate
-l cpp
-c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
-o ${CMAKE_CURRENT_BINARY_DIR}/arora)
add_custom_target(generate-nao ALL
  COMMAND ${ARORA_MODULE_CLI}
  --include ${ARORA_BEHAVIOR_TREE_INCLUDE}
  generate
  -l cpp
  -c ${CMAKE_CURRENT_SOURCE_DIR}/module.yaml
  -o ${CMAKE_CURRENT_BINARY_DIR}/arora
  BYPRODUCTS
    ${CMAKE_CURRENT_BINARY_DIR}/arora/include/nao.hpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/source/nao.cpp
    # ${CMAKE_CURRENT_BINARY_DIR}/arora/include/types/Status.hpp
    # ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
    ${CMAKE_CURRENT_BINARY_DIR}/arora/module.yaml
    ${CMAKE_CURRENT_BINARY_DIR}/arora/records/module/ed2a9b9b-a2b8-438e-a99c-29e0df09c1d4.yaml
)

add_library(
  nao SHARED
  src/nao.cpp
  # Auto-generated sources for this module.
  ${CMAKE_CURRENT_BINARY_DIR}/arora/source/nao.cpp
  # ${CMAKE_CURRENT_BINARY_DIR}/arora/source/types/Status.cpp
)

# Makes Qi available.
# Also makes OpenSSL and Boost available.
include(FetchContent)
FetchContent_Declare(
  qi
  GIT_REPOSITORY https://github.com/semio-ai/libqi.git
  GIT_TAG 5663c31d674a32c5ec208864ca5acb802e425c40
)
FetchContent_MakeAvailable(qi)

# Other arora dependencies.
set(ARORA_INCLUDE_DIR ${ARORA_BINARY_DIR}/../../include)
add_library(arora-buffers STATIC IMPORTED GLOBAL)
set_property(TARGET arora-buffers PROPERTY IMPORTED_LOCATION ${ARORA_BINARY_DIR}/libarora_buffers.a)
set_property(TARGET arora-buffers PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_INCLUDE_DIR})

add_library(arora-util STATIC IMPORTED GLOBAL)
set_property(TARGET arora-util PROPERTY IMPORTED_LOCATION ${ARORA_BINARY_DIR}/libarora_util.a)
set_property(TARGET arora-util PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${ARORA_INCLUDE_DIR})

add_subdirectory(arora-c) # exports arora-cpp
add_subdirectory(arora-cpp) # exports arora-cpp

# Includes and linking.
add_dependencies(nao qi generate-nao)

target_include_directories(
  nao PRIVATE
  # LibQi includes.
  ${QI_INCLUDE_DIRS}
  # Arora includes.
  ${ARORA_INCLUDE_DIR}
  ${ARORA_BUFFERS_CPP_INCLUDE_DIR}
  # Auto-generated headers for this module.
  ${CMAKE_CURRENT_BINARY_DIR}/arora/include
)
target_link_libraries(
  nao PRIVATE
  # LibQi libraries.
  qi
  ${CMAKE_DL_LIBS}
  # Arora libraries.
  arora-buffers
  arora-util
  arora-cpp
)

# Threads
find_package(Threads)
if(Threads_FOUND)
  target_link_libraries(nao PRIVATE Threads::Threads ${CMAKE_THREAD_LIBS_INIT})
else() # No threads library on Mac or Android
  target_link_libraries(nao PRIVATE -pthread)
endif()

