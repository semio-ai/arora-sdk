

add_library(arora-c INTERFACE)
target_include_directories(arora-c INTERFACE "${TARGET_DIR}/include")

install(
  FILES
    "${NATIVE_LIB_DIR}/libarora${CMAKE_SHARED_LIBRARY_SUFFIX}"
    "${NATIVE_LIB_DIR}/libarora_buffers${CMAKE_SHARED_LIBRARY_SUFFIX}"
    "${NATIVE_LIB_DIR}/libarora_util${CMAKE_SHARED_LIBRARY_SUFFIX}"
  DESTINATION lib
)

install(DIRECTORY "${TARGET_DIR}/include/arora" DESTINATION include)