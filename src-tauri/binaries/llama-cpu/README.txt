CPU-only build of llama.cpp's llama-server, the fallback when the GPU build in
../llama cannot start (no Vulkan driver). CI fills this folder on Windows and
Linux x64; see ../llama/README.txt. Bundled as the `llama-cpu/` resource.
