#!/bin/sh
# Rebuilds the two small assemblies the crate carries (`include_bytes!`):
# the UnityEngine stand-in and the driver. Needs Mono's `mcs` and REPO's
# Klattersynth.dll to compile the driver against (it is not copied):
#   crates/repo_tts/dotnet/build.sh <REPO>/REPO_Data/Managed/Klattersynth.dll
set -e
cd "$(dirname "$0")"
mcs -target:library -out:UnityEngine.dll UnityEngine.cs
mcs -target:library -r:UnityEngine.dll -r:"$1" -out:Bo2Tts.dll Bo2Tts.cs
