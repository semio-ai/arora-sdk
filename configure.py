#!/usr/bin/python3
import argparse
from genericpath import exists
from os import getcwd, makedirs, path, stat
import platform
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

wasi_version = "14"
wasi_version_full = f"{wasi_version}.0"
wasi_platform = ""
cmake_extra_args = list()
make_program = "make"

if platform.system() == "Windows":
  wasi_platform = "mingw"
  cmake_extra_args = ["-DCMAKE_MAKE_PROGRAM=ninja", "-G Ninja"]
  make_program = "ninja"
elif platform.system() == "Linux":
  wasi_platform = "linux"
elif platform.system() == "Darwin":
  wasi_platform = "macos"


url = f"https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-{wasi_version}/wasi-sdk-{wasi_version_full}-{wasi_platform}.tar.gz"

tmp = tempfile.gettempdir()

print(f"Downloading WASI SDK...")
sdk_archive = path.join(tmp, f"wasi-sdk-{wasi_version}.tar.gz")
urllib.request.urlretrieve(url, sdk_archive)

print(f"Extracting WASI SDK...")
tarfile.open(sdk_archive, "r:gz").extractall()

current_dir = getcwd().replace("\\", "/")
wasi_sdk_prefix = f"{current_dir}/wasi-sdk-14.0"
print(f"WASI SDK was put in {wasi_sdk_prefix}")

print(f"Creating 'build' directory...")
# Check if build directory already exists
if not exists(f"build"):
  makedirs(f"build")

print(f"Running cmake...")
toolchain_file = path.join(wasi_sdk_prefix, "share", "cmake", "wasi-sdk.cmake")
# CMake misinterprets backslashes on Windows, let's make its life easier by providing slashes.
wasi_sdk_prefix = wasi_sdk_prefix.replace('\\', '/')
toolchain_file = toolchain_file.replace('\\', '/')
cmake_command = [
    "cmake",
    "..",
    f"-DWASI_SDK_PREFIX={wasi_sdk_prefix}",
    f"-DCMAKE_TOOLCHAIN_FILE={toolchain_file}",
  ]
cmake_command.extend(cmake_extra_args)
if len(sys.argv) > 1:
  cmake_command.extend(sys.argv[1:])
print(' '.join(cmake_command))
subprocess.check_call(cmake_command, cwd="build")

print(f"Configuration complete. Run '{make_program}' in the build folder to build arora.")
