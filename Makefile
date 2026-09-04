PREFIX ?= $(HOME)/.local


install:
	@mkdir -p $(PREFIX)/bin
	install -m 755 mw.sh $(PREFIX)/bin/mw


uninstall:
	rm -f $(PREFIX)/bin/mw
