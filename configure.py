#!/usr/bin/python3

from genericpath import exists
import subprocess
from os import getcwd, makedirs, stat
import platform
import tempfile
import urllib.request
import tarfile


wasi_version = "14"
wasi_version_full = f"{wasi_version}.0"
wasi_platform = ""

if platform.system() == "Windows":
  wasi_platform = "mingw"
elif platform.system() == "Linux":
  wasi_platform = "linux"
elif platform.system() == "Darwin":
  wasi_platform = "macos"


url = f"https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-{wasi_version}/wasi-sdk-{wasi_version_full}-{wasi_platform}.tar.gz"

tmp = tempfile.gettempdir()

print(f"Downloading WASI SDK...")
urllib.request.urlretrieve(url, f"{tmp}/wasi-sdk-{wasi_version}.tar.gz")

print(f"Extracting WASI SDK...")
tarfile.open(f"{tmp}/wasi-sdk-{wasi_version}.tar.gz", "r:gz").extractall()

current_dir = getcwd()

print(f"Creating build directory...")
# Check if build directory already exists
if not exists(f"build"):
  makedirs(f"build")

print(f"Running cmake...")
subprocess.run(["cmake", "..", f"-DWASI_SDK_PREFIX={current_dir}/wasi-sdk-14.0", f"-DCMAKE_TOOLCHAIN_FILE={current_dir}/cmake/WASI.toolchain"], cwd="build")


printf("Configuration complete. Run 'make' in the build folder to build arora.")