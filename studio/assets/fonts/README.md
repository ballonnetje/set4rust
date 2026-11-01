# Fonts

* `Inter-*.ttf`: [Inter](https://github.com/rsms/inter) 4.1, static instances from `extras/ttf`, SIL Open
  Font License 1.1 (`Inter-LICENSE.txt`).
* `Phosphor.ttf`: [Phosphor Icons](https://github.com/phosphor-icons/core), regular weight, MIT
  (`Phosphor-LICENSE.txt`). Modified: every character map entry outside the private use area
  (U+E000..U+F8FF) and the GSUB, GPOS and GDEF tables are removed. The original also maps Latin
  letters (for ligatures), and Inter claims private-use code points of its own, so neither order in
  a font stack works with the two files as published. Icon code points are listed in
  `src/theme.rs`.
