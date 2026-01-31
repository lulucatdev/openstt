.PHONY: install dev dev-auto build tauri-build tauri-dev lint fmt cargo-build test models-dir download-tiny

install:
	npm install

dev:
	npm run tauri dev

dev-auto:
	OPENSTT_AUTOSTART=1 OPENSTT_DEFAULT_MODEL=base npm run tauri dev

build:
	npm run build

tauri-build:
	npm run tauri build

tauri-dev:
	npm run tauri dev

lint:
	npm run lint

fmt:
	npm run format

cargo-build:
	cd src-tauri && cargo build

test:
	npm test

models-dir:
	@echo "$$HOME/.openstt/models"

download-tiny:
	@mkdir -p "$$HOME/.openstt/models/whisper"
	@curl -L "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin" \
		-o "$$HOME/.openstt/models/whisper/ggml-tiny.bin"
