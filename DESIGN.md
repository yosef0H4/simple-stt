# Settings design

Approved direction: compact visible rows, icons with short navigation names, all Settings pages. Operate mode: users configure dictation, not read a dashboard.

Use flat sections with subtle dividers, aligned controls, a compact sidebar, and a single restrained blue accent. Neutral light/dark surfaces use the system font stack. Section headings have small accent SVG icons, with no enclosing tile or badge. Controls carry short labels; optional explanations appear through help icons. Do not add oversized cards, decorative icon tiles, imagery, gradients, or persistent animation.

Desktop navigation shows names and icons; narrow screens show accessible icons. Mount only the active page. Save remains labeled and appears in a compact footer for modified drafts. Search uses metadata across all pages. Dropdowns escape their containers and support keyboard input. Every control has focus, hover, disabled and pending states; status/error messages remain concise and actionable.

Use consistent authored SVG strokes, 36px form controls, 34px action buttons, a 14px base font, and a fixed heading scale. Respect reduced motion and light/dark/system preferences. Preserve Arabic names and direction in user content.

Verification: review both themes and 360/768/1280px layouts in a single batch, fix reported defects together, and confirm once. Frontend asset budgets and measured memory/startup comparisons are recorded in the test workflow.
