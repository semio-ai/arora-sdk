# Write a module directory: run by arora_module_directory() (CMakeLists.txt)
# as `cmake -P`, with
#
#   DIRECTORY  the module directory
#   NAME       the guest's name: it is written as <NAME>.wasm
#   GUEST      the linked guest
#   HEADER     the generated header, written as header.json
#   RECORDS    the records directories, separated by `|`, merged into records/
#
# It never removes the directory, copies only the files whose content differs,
# and removes from records/ only the files no RECORDS directory holds. Two runs
# writing one directory at once — cargo runs a C++ module's build script once
# per dependency kind it is built as — so agree, and neither removes a file
# the other still writes or reads.

cmake_minimum_required(VERSION 3.17)

foreach(var DIRECTORY NAME GUEST HEADER)
  if(NOT DEFINED ${var})
    message(FATAL_ERROR "arora-module-directory.cmake: ${var} is not set")
  endif()
endforeach()

function(copy_if_different from to)
  get_filename_component(parent "${to}" DIRECTORY)
  file(MAKE_DIRECTORY "${parent}")
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${from}" "${to}"
    RESULT_VARIABLE result)
  if(NOT result EQUAL 0)
    message(FATAL_ERROR "could not copy ${from} to ${to}")
  endif()
endfunction()

file(MAKE_DIRECTORY "${DIRECTORY}/records")
copy_if_different("${HEADER}" "${DIRECTORY}/header.json")
copy_if_different("${GUEST}" "${DIRECTORY}/${NAME}.wasm")

string(REPLACE "|" ";" RECORDS "${RECORDS}")
set(wanted "")
foreach(records IN LISTS RECORDS)
  file(GLOB_RECURSE files RELATIVE "${records}" "${records}/*")
  foreach(record IN LISTS files)
    copy_if_different("${records}/${record}" "${DIRECTORY}/records/${record}")
    list(APPEND wanted "${record}")
  endforeach()
endforeach()

file(GLOB_RECURSE present RELATIVE "${DIRECTORY}/records" "${DIRECTORY}/records/*")
foreach(record IN LISTS present)
  if(NOT record IN_LIST wanted)
    file(REMOVE "${DIRECTORY}/records/${record}")
  endif()
endforeach()
