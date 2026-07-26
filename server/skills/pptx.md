+++
name = "pptx"
title = "Working with PowerPoint presentations"
description = "Read, write, modify PowerPoint documents"
+++

# General working with PowerPoint

If you have access to a Python interpreter, check python-pptx package. Here's an example of how to work with presentations via this package:

```
from pptx import Presentation
from pptx.util import Inches, Pt
from pptx.dml.color import RGBColor
from pptx.enum.text import PP_ALIGN

# --- Color palette ---
DARK   = RGBColor(0x1B, 0x1B, 0x2F)
WHITE  = RGBColor(0xFF, 0xFF, 0xFF)
ACCENT = RGBColor(0x00, 0x96, 0xC7)

# --- Create presentation (16:9 widescreen) ---
prs = Presentation()
prs.slide_width  = Inches(13.333)
prs.slide_height = Inches(7.5)

# ---- Helper functions ----
def add_bg(slide, color=DARK):
    fill = slide.background.fill
    fill.solid()
    fill.fore_color.rgb = color

def add_textbox(slide, left, top, width, height, text,
                size=18, color=WHITE, bold=False, align=PP_ALIGN.LEFT):
    tx = slide.shapes.add_textbox(left, top, width, height)
    tf = tx.text_frame
    tf.word_wrap = True
    p = tf.paragraphs[0]
    p.text = text
    p.font.size = Pt(size)
    p.font.color.rgb = color
    p.font.bold = bold
    p.alignment = align
    return tx

def add_bullets(slide, left, top, width, height, items,
                size=18, color=WHITE):
    tx = slide.shapes.add_textbox(left, top, width, height)
    tf = tx.text_frame
    tf.word_wrap = True
    for i, item in enumerate(items):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        p.text = item
        p.font.size = Pt(size)
        p.font.color.rgb = color
        p.space_after = Pt(10)
    return tx

def add_accent_bar(slide, top=Inches(1.4)):
    shape = slide.shapes.add_shape(
        1, Inches(0.8), top, Inches(1.2), Inches(0.07)
    )
    shape.fill.solid()
    shape.fill.fore_color.rgb = ACCENT
    shape.line.fill.background()

def add_prompt_box(slide, left, top, width, height, label, text):
    """A colored card with a label and body text."""
    box = slide.shapes.add_shape(1, left, top, width, height)
    box.fill.solid()
    box.fill.fore_color.rgb = RGBColor(0x26, 0x26, 0x40)
    box.line.color.rgb = ACCENT
    box.line.width = Pt(1)
    tf = box.text_frame
    tf.word_wrap = True
    tf.margin_left  = Inches(0.15)
    tf.margin_right = Inches(0.15)
    tf.margin_top   = Inches(0.1)
    p = tf.paragraphs[0]
    p.text = label
    p.font.size = Pt(12)
    p.font.bold = True
    p.font.color.rgb = ACCENT
    p2 = tf.add_paragraph()
    p2.text = text
    p2.font.size = Pt(15)
    p2.font.color.rgb = WHITE
    p2.space_before = Pt(4)

# ---- Slide 1: Title ----
s = prs.slides.add_slide(prs.slide_layouts[6])
add_bg(s, DARK)
add_textbox(s, Inches(0.8), Inches(2.3), Inches(11.7), Inches(1.2),
            "My Presentation", size=48, color=WHITE, bold=True,
            align=PP_ALIGN.CENTER)
add_accent_bar(s, Inches(3.6))
add_textbox(s, Inches(0.8), Inches(3.9), Inches(11.7), Inches(0.6),
            "Built with python-pptx", size=20, color=ACCENT,
            align=PP_ALIGN.CENTER)

# ---- Slide 2: Bullets + prompt card ----
s = prs.slides.add_slide(prs.slide_layouts[6])
add_bg(s, DARK)
add_textbox(s, Inches(0.8), Inches(0.5), Inches(11.7), Inches(0.8),
            "Key Points", size=36, color=WHITE, bold=True)
add_accent_bar(s, Inches(1.25))
add_bullets(s, Inches(0.8), Inches(1.8), Inches(11.7), Inches(3), [
    "Reproducible: generate decks from data or scripts",
    "Full control: every shape, color, and font is code",
    "Scales: build 100-slide decks as easily as 2 slides",
], size=22, color=WHITE)
add_prompt_box(s, Inches(0.8), Inches(4.8), Inches(11.7), Inches(1.8),
               "Example Prompt",
               "Create a 5-slide pitch deck for my coffee subscription startup.")

# ---- Save ----
prs.save("/home/agent/output.pptx")
print("Saved /home/agent/output.pptx")
```

# Creating presentations

## Design Ideas

If user didn't specify a template or requirements use guidelines below to ground yourself.

**Don't create boring slides.** Plain bullets on a white background won't impress anyone. Consider ideas from this list for each slide.

### Before Starting

- **Pick a bold, content-informed color palette**: The palette should feel designed for THIS topic. If swapping your colors into a completely different presentation would still "work," you haven't made specific enough choices.
- **Dominance over equality**: One color should dominate (60-70% visual weight), with 1-2 supporting tones and one sharp accent. Never give all colors equal weight.
- **Dark/light contrast**: Dark backgrounds for title + conclusion slides, light for content ("sandwich" structure). Or commit to dark throughout for a premium feel.
- **Commit to a visual motif**: Pick ONE distinctive element and repeat it — rounded image frames, icons in colored circles, thick single-side borders. Carry it across every slide.

### Color Palettes

Choose colors that match your topic — don't default to generic blue. Use these palettes as inspiration:

| Theme | Primary | Secondary | Accent |
|-------|---------|-----------|--------|
| **Midnight Executive** | `1E2761` (navy) | `CADCFC` (ice blue) | `FFFFFF` (white) |
| **Forest & Moss** | `2C5F2D` (forest) | `97BC62` (moss) | `F5F5F5` (cream) |
| **Coral Energy** | `F96167` (coral) | `F9E795` (gold) | `2F3C7E` (navy) |
| **Warm Terracotta** | `B85042` (terracotta) | `E7E8D1` (sand) | `A7BEAE` (sage) |
| **Ocean Gradient** | `065A82` (deep blue) | `1C7293` (teal) | `21295C` (midnight) |
| **Charcoal Minimal** | `36454F` (charcoal) | `F2F2F2` (off-white) | `212121` (black) |
| **Teal Trust** | `028090` (teal) | `00A896` (seafoam) | `02C39A` (mint) |
| **Berry & Cream** | `6D2E46` (berry) | `A26769` (dusty rose) | `ECE2D0` (cream) |
| **Sage Calm** | `84B59F` (sage) | `69A297` (eucalyptus) | `50808E` (slate) |
| **Cherry Bold** | `990011` (cherry) | `FCF6F5` (off-white) | `2F3C7E` (navy) |

### For Each Slide

**Every slide needs a visual element** — image, chart, icon, or shape. Text-only slides are forgettable.

**Layout options:**
- Two-column (text left, illustration on right)
- Icon + text rows (icon in colored circle, bold header, description below)
- 2x2 or 2x3 grid (image on one side, grid of content blocks on other)
- Half-bleed image (full left or right side) with content overlay

**Data display:**
- Large stat callouts (big numbers 60-72pt with small labels below)
- Comparison columns (before/after, pros/cons, side-by-side options)
- Timeline or process flow (numbered steps, arrows)

**Visual polish:**
- Icons in small colored circles next to section headers
- Italic accent text for key stats or taglines

### Typography

**Choose an interesting font pairing** — don't default to Arial. Pick a header font with personality and pair it with a clean body font.

| Header Font | Body Font |
|-------------|-----------|
| Georgia | Calibri |
| Arial Black | Arial |
| Calibri | Calibri Light |
| Cambria | Calibri |
| Trebuchet MS | Calibri |
| Impact | Arial |
| Palatino | Garamond |
| Consolas | Calibri |

| Element | Size |
|---------|------|
| Slide title | 36-44pt bold |
| Section header | 20-24pt bold |
| Body text | 14-16pt |
| Captions | 10-12pt muted |

### Spacing

- 0.5" minimum margins
- 0.3-0.5" between content blocks
- Leave breathing room—don't fill every inch

### Avoid (Common Mistakes)

- **Don't repeat the same layout** — vary columns, cards, and callouts across slides
- **Don't center body text** — left-align paragraphs and lists; center only titles
- **Don't skimp on size contrast** — titles need 36pt+ to stand out from 14-16pt body
- **Don't default to blue** — pick colors that reflect the specific topic
- **Don't mix spacing randomly** — choose 0.3" or 0.5" gaps and use consistently
- **Don't style one slide and leave the rest plain** — commit fully or keep it simple throughout
- **Don't create text-only slides** — add images, icons, charts, or visual elements; avoid plain title + bullets
- **Don't forget text box padding** — when aligning lines or shapes with text edges, set `margin: 0` on the text box or offset the shape to account for padding
- **Don't use low-contrast elements** — icons AND text need strong contrast against the background; avoid light text on light backgrounds or dark text on dark backgrounds
- **NEVER use accent lines under titles** — these are a hallmark of AI-generated slides; use whitespace or background color instead

