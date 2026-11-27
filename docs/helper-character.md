# Audra's helper character (idea notes)

The owner, 2026-11-25, for the next set of features: a cute animated helper in the spirit of Office's old paperclip assistant, replacing the tips that show at the bottom of the window. Audra is in bug-closing mode until then, so these are notes, not work.

Rules for it:
- **Audra's own, original character.** Clippy is a reference for the behaviour only (a helper with a speech bubble and choices), never for its looks, name or lines.
- **Never in the way.** It can be switched off, it never covers controls or the meters, and safety warnings (open mic, levels) stay where they are, not in its bubble.
- **No AI in it today** (see the Later list: an AI assistant needs its own design talk first).

## What the Terminal launcher's cat (Byte) does, from that session's notes

Byte is drawn in code (cairo), with no image files.
- **Poses** are values from 0 to 1 (blink per eye, ear twitch, tail swing, hop, raised paw, look direction, mouth shape, blush, sleep, walk phase, facing).
- **Each move** is a short sin(πt) envelope layered on the resting pose, so moves overlap: blink 0.18 s, wink 0.6 s, hop with blush 0.5 s, wave 1.1 s, nod 0.5 s.
- **Idle:** a tick once a second (about 22% blink, about 3% a random wink, tail, ear or look), asleep from 23:00 to 06:00. The frame timer runs only while something moves.
- **Roaming:** it roams in an overlay along routes that never cross content (down to the floor, along it, straight up). Only the cat takes clicks, and the pointer resting on it makes it step aside.
- **Bubbles:** a greeting with the date on open, and every 45 s a cycle of word, joke, tip and greeting. It also reacts to actions (a star, a badge, a build finished or failed).
- **Hold times:** reactions about 8 s, tips about 14 s, idle lines 10 s, then the bubble fades. Tips show once ever.
- **One voice per moment:** the cat replaces toasts for tips and badges; toasts stay for errors and buttons. At most one extra sound per action, 450 ms after the action's own sound; a badge beats a tip; small comments wait if it spoke less than 8 s ago.
- **Switches:** sounds, tips, the cat itself (hidden, its timers stopped). It follows the desktop's animation and sound settings.
- **That owner's wishlist:** a wobble when poked, with a bouncing bubble; Clippy-style bubbles with clickable choices ("Would you like help?" / don't show again); bigger; sounds (purr, babble); more expressive eyes and body (mood eyes, squash and stretch).
- **Lesson learned:** a toast once covered its line, so messages moved into the bubble.

## The owner's pick (2026-11-26)

From four objects (knob, cassette, mic, meters: "none of them, I want it to be closer to a cat") and then three cats (Tux in headphones, Cable with a jack-plug tail, Speaker): **the grey tabby sitting on a little speaker**. Built in `crates/app/src/helper.rs`: beside every foot card (tips, milestones, hints) as its speech bubble; blinks, glances between you and the bubble, hops when a card comes, smiles wider for a milestone; the woofer pulses with Main's level; still with Animations off; its own switch, Audra settings → App → "A cat beside the tips".

Next ideas (not built): idle and greeting lines (Lemonade's draft in target/helper/lines-draft.md, each line to be checked), dozing at night, "Would you like help?" bubbles with choices, a wobble when clicked, purring through the UI sounds.

## What fits Audra (first thoughts, before the pick)

- **What it says** is what the tips say now: Audra's existing tips, guide steps and celebrations, with "Got it" and "Don't show again" as clickable choices in the bubble.
- **Character ideas,** each to be drawn in Audra's own style (iced canvas, like the knobs and meters) and shown as pictures to pick from:
  - a little knob with eyes;
  - a cassette/tape reel;
  - a small microphone;
  - a VU-meter needle face.
- **Reacting to the mix:** it could react to wins the app already celebrates (first sound, a guide done), hum along when the meters move, and doze when nothing plays.
- **Where it lives:** the corner where the tips sit now, peeking in from the edge, never crossing the strips.
- **Sounds:** made with Audra's own UI sound engine, so they follow the UI sound switch and volume.
