PREFIX ?= $(HOME)/.local
BIN := target/release/mw


build:
	cargo build --release


install: build
	@mkdir -p $(PREFIX)/bin
	install -m 755 $(BIN) $(PREFIX)/bin/mw


uninstall:
	rm -f $(PREFIX)/bin/mw