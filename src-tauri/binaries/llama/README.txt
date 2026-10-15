Built-in model runtime (llama.cpp's llama-server and its libraries).

CI fills this folder from the llama.cpp GitHub release during the build
(.github/workflows/build.yml, step "Fetch the built-in model runtime") and
Tauri bundles it as the `llama/` resource. For a local build, download the
release zip for your platform from https://github.com/ggml-org/llama.cpp/releases,
copy llama-server and the shared libraries next to this file, and write the
release tag into a file named VERSION. Without it the app still works; the
assistant then offers Ollama, LM Studio and cloud providers only.
