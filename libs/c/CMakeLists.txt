

add_library(arora-c INTERFACE)
target_include_directories(arora-c INTERFACE "${TARGET_DIR}/include")
target_link_libraries(arora-c INTERFACE arora-buffers arora-util)

install(
  FILES
    "${RUST_BINARY_DIR}/libarora_buffers${CMAKE_STATIC_LIBRARY_SUFFIX}"
    "${RUST_BINARY_DIR}/libarora_util${CMAKE_STATIC_LIBRARY_SUFFIX}"
  DESTINATION lib
)

install(DIRECTORY "${RUST_TARGET_DIR}/include/arora" DESTINATION include)