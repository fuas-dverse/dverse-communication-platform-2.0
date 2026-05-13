LATEX      := pdflatex
LATEXFLAGS := -interaction=nonstopmode -halt-on-error
OUTDIR     := build

TEXFILES   := $(wildcard *.tex)
PDFS       := $(patsubst %.tex,$(OUTDIR)/%.pdf,$(TEXFILES))

.PHONY: all clean help

all: $(OUTDIR) $(PDFS)

$(OUTDIR):
	mkdir -p $(OUTDIR)

$(OUTDIR)/%.pdf: %.tex | $(OUTDIR)
	cp $< $(OUTDIR)/$(notdir $<)
	cd $(OUTDIR) && $(LATEX) $(LATEXFLAGS) $(notdir $<)
	cd $(OUTDIR) && $(LATEX) $(LATEXFLAGS) $(notdir $<)
	rm -f $(OUTDIR)/$(patsubst %.tex,%.aux,$(notdir $<)) \
	      $(OUTDIR)/$(patsubst %.tex,%.log,$(notdir $<)) \
	      $(OUTDIR)/$(patsubst %.tex,%.out,$(notdir $<)) \
	      $(OUTDIR)/$(notdir $<)

clean:
	rm -rf $(OUTDIR)

help:
	@echo "Usage:"
	@echo "  make        - build all ADR PDFs into ./build/"
	@echo "  make clean  - remove ./build/"
	@echo "  make ADR-001 - build a single ADR by name (no extension)"

%: $(OUTDIR)/%.pdf
	@true