#set page(
  paper: "a4",
  margin: (x: 22mm, y: 20mm),
  footer: context [
    #align(center)[#counter(page).display()]
  ],
)
#set text(font: "Libertinus Serif", size: 10.5pt)
#set heading(numbering: "1.")
#show heading.where(level: 1): it => block(
  above: 1.5em,
  below: 0.8em,
  text(size: 18pt, weight: "bold", it),
)

= Report title

Start the report here.
