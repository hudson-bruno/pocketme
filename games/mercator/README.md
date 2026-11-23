# mercator: MERCATOR

You fly a small trading ship through the Lens, a galaxy of 136 star
systems. Buy goods cheap where they are made, jump to a system that
wants them, and sell dear. Raiders hunt you in the dangerous systems;
wardens keep the peace and come after anyone who shoots a hauler or a
warden (you are then an OUTLAW until you leave the system).

The game's story is a map. The Mapmakers' Guild pays a survey fee the
first time you dock at a system, and it grants its charter to a pilot
who has surveyed 20 systems and docks with 50,000 credits: then you
have won, and the flight goes on. Your standing grows with your
surveys: DRIFTER, SKETCHER, PLOTTER, CHARTWRIGHT, MAPMAKER.

Fit a better laser, stronger shields and a sun skimmer, and trade your
ship in for a cargo ship or a combat ship (step M5).
A second player on pad 2 is the gunner (see below).

Everything is real 3D: the sun, the planet, the station and every ship
are flat-shaded triangles, turned and seen in perspective, in whole
numbers. The polygon code (`src/poly.rs`, `src/fixed.rs`) is KESTREL's,
copied and trimmed. KESTREL itself is unchanged.

All of it is original: the name, the galaxy's shape, the systems, the
goods, the ships, the station, the rules, the wording and the screens
were made for this game.

## The galaxy

The Lens is 12 rows of 5 to 15 cells, each row centred, one system per
cell (136 in all). A cell is 3.4 x 2.8 light years, so every neighbour
is within a full tank (6.5 light years) and every system can be
reached. Each galaxy comes from one seed number; nothing is stored.
Every system has a made-up name of two or three syllables (no two the
same in a galaxy), an economy (FARM, MINE, FORGE, LAB or HAVEN), a tech
level and a danger level (SECURE, QUIET, RISKY or LAWLESS); the Lens's
rim is rougher. Prices follow the economy, change a little from day to
day, and a jump takes a day. The title menu picks one of 10 galaxies.

**Risk pays.** Few traders dare to fly to dangerous systems, so their
markets swing wider: what the economy makes is cheaper there and what
it wants is dearer. A SECURE system has the plain prices; each danger
step (QUIET, RISKY, LAWLESS) widens them a little more (`RISK_PAY` in
`src/market.rs`). Buying in a LAWLESS system and selling in another
pays about twice what the same run between two SECURE systems pays;
goods an economy neither makes nor wants keep their price. The market
screen of a RISKY or LAWLESS system says RISK PAY.

## How to play

| Screen | What you do |
|---|---|
| Title | PLAY (CONTINUE while a game runs), NEW GAME, GALAXY, DETAIL (LOW, MID, HIGH), VIEW (COCKPIT, CHASE), PICTURES (30 or 60 A SECOND), PITCH (NORMAL, INVERTED), LEAVE. |
| Docked | LAUNCH, MARKET, OUTFIT, CHART, SHIPYARD; your standing, the ship you fly and its hull, surveys (of the 20 the charter needs) and the charter's sum; while you carry raider TAGs not yet paid, a line above them (TAGS 4: 325 CR). A pilot who docks with an empty hold and too little money for a tank gets the tank filled free, so nobody is stranded. |
| Market | 8 goods. A or right buys a tonne, X or left sells one; a tap on a row's BUY or SELL column does the same. Hold the key (or keep the finger there) to go on buying or selling, faster the longer you hold. Green goods are made here (cheap); RISK PAY marks a RISKY or LAWLESS market, where prices swing wider. |
| Outfit | FUEL (fill the tank), REPAIR (mend the hull), LASER MK2 and MK3, SHIELD MK2 and MK3, SUN SKIMMER. Higher tech systems sell more gear. |
| Shipyard | The three ships (LARK, DRAY, SHRIKE) with hold, speed, hull and cost; the picked one turns in a box on the right, with its list price, turn rate, shield, overboost and what your ship is worth. A buys (a tap selects a row, a second tap on it buys). Higher tech systems sell more ships. |
| Chart | The systems in fuel range, with economy and danger; surveyed systems are bigger dots, and the line under the map shows the survey fee (SURVEY +75) or SURVEYED. X zooms out to the whole Lens. A sets the course; in flight, A jumps. |
| Flight | Fly to the station and dock, fight, scoop cargo and raider TAGs, or open the chart and jump. While you carry TAGs the panel shows how many, in amber at the right end of AUTO's row, with a clear gap after AUTO. |

**Starting settings.** The game starts at DETAIL HIGH, VIEW CHASE and
PICTURES 60 A SECOND (the owner's choice, 2026-11-23; before that it
started at MID and COCKPIT). Every option stays in the title and pause
menus, and the settings are not saved: each start of the game begins
with these (`App::blank` in `src/app.rs`).

Flight controls (pad):

| Control | What it does |
|---|---|
| Left stick or d-pad | pitch and yaw |
| Right stick across, or L and R | roll |
| Right trigger / left trigger, right stick up and down, X / B | faster / slower |
| X at full speed | cruise (fast travel; blocked near the station and near other ships) |
| X at full speed where cruise is blocked, kept held | OVERBOOST: 7/4 of top speed while the charge lasts (3 s full), half the turn rate, no firing |
| A | fire the laser |
| Y | auto-dock on or off |
| START | pause menu (RESUME, DETAIL, VIEW, PICTURES, PITCH, TITLE) |
| SELECT | chart |
| Pad 2, A held | the gunner fires the dorsal turret |

**Docking.** Fly into the station's mouth facing in and slowly; the
station's turn does not matter. Or use the tow lane: come within about
2.6 km of the mouth (3 km of the station's middle), near its axis, facing in and not faster than top speed,
and the station's tow takes you in (once a flight; Y lets go). The
auto-dock (Y) flies the whole way from anywhere when no one hunts you.

**The gunner.** Player 2's pad mans a turret on the ship's spine. While
A is held it aims by itself at a raider (or a ship you have shot, not
a canister)
within 1.5 km, in any direction, and fires: weaker than the nose laser,
and it heats the same laser coolant, so pilot and gunner share the heat.

**The panel.** Left: the scanner, a round scope (middle ahead, rim
behind) with the station and every ship within 12 km. Middle: speed
(with the throttle's mark) and under it a one-pixel line, the
overboost's charge, then shield, hull, heat, fuel, credits. Right:
AUTO (lit while the auto-dock or the tow flies) and the charter's two
gauges, SVY (surveys of 20) and FND (credits of 50,000). While you
carry raider TAGs, their number stands in amber on AUTO's row, ending
where the SVY and FND bars end (x 157): 1 to 9 at x 155, 10 to 99 at
x 151, 100 to 255 at x 147, so at least 5 empty pixel columns (more
than a word space) stay between AUTO and the number. On the view,
the station is bracketed; when it is off the view a beacon on the
view's edge shows which way to turn.

**Holding in the market.** The first press trades one tonne at once.
Held, the key trades again after 0.45 s, then 6 a second, getting
faster until 20 a second after 1.5 s: about 11 tonnes in 1.5 s, 21 in
2 s, 40 tonnes in 3 s. The speed is counted in game time, so it is the
same at 30 and at 60 pictures a second. It stops at once when you let
go or move to another row, and by itself, with one note, when the hold
is full, the money or the station's stock runs out, or there is
nothing left to sell. A buy key and a sell key held together repeat
nothing. Fuel and repairs fill up in one press, and gear is never
bought by holding.

**Range on the view.** The laser reaches 2.4 km. The two views mark
it alike but for one thing: the COCKPIT view draws a small crosshair
in the view's middle, and the CHASE view (the starting view, from
behind and above the ship) draws none. The brackets, boxes, target
line and bar below are on the view in both. A line at the foot of
the view names one ship and its distance, for example RAIDER 1.6 KM
or HAULER 914 M (whole metres under 1 km, tenths of a km above,
rounded down): the ship a shot would hit now, else the ship nearest
the view's middle. A short bar under the ship shows which one it is.
When the laser reaches that ship the line is amber and ends in IN RANGE.
A raider's TAG is the one exception: the laser passes through it, so
its line never says IN RANGE. It ends in FLY INTO IT at any distance
(RAIDER TAG 420 M  FLY INTO IT), and its line, its bar and its four
corners are magenta (`art::TAG_MARK`) at every distance, never amber.
A ship is named only while it is drawn (within 7 km), so a TAG's line
is at most 124 pixels wide in play (130 at the longest distance text
there is) and stays clear of the station's beacon when that sits in
one of the view's lower corners. The beacon can also sit on the view's
bottom edge (the station below or behind the ship: y 88, anywhere from
x 6 to 154), on the line's own row; the line is drawn after the beacon,
so its letters stay whole and the beacon shows round them. Other bracketed ships (raiders hunting you, cargo canisters)
get a closed amber box instead of their four corners while they are
within reach. In the COCKPIT view the
crosshair turns amber, with a small box in its middle, when pulling
the trigger now would hit a ship; it is red while the laser is too hot
to fire. All of these use the laser's own rule (`World::in_reach` and
`World::first_hit` in `src/world.rs`): within 2.4 km, in front, and
the aim line passing within the ship's size plus 1/40 of its distance.
The CHASE view has no crosshair to lock: there the target line names
the ship a shot would hit, when there is one. In both views the
panel's HEAT bar is red while the laser is too hot to fire.
Any ship can be hit, wardens and haulers too, so the COCKPIT crosshair
also locks on them; the line says which kind it is.

Touch: a finger in the view steers (and fires when a foe is in the
sights). On the panel below: the scanner opens the chart, then slower,
faster, and AUTO for the auto-dock. At top speed a new touch on the
faster bars is the X press (cruise), and where cruise is blocked the
finger kept there overboosts, as X held does. Menu lines can be
tapped. A tap on the top line pauses.

The laser heats up with every shot; too hot and it stops until it
cools. Shields soak up hits and come back slowly; the hull does not.
Flying close to the sun heats the ship, and with a sun skimmer it
refills the tank. Shot ships may drop cargo canisters: fly into one to
scoop it. In SECURE and QUIET systems no raiders appear within 8 km of
the station.

**Haulers (step F1).** A hauler has 540 hull points (68, 39 or 25 hits
of the MK1, MK2 or MK3 laser). When your shot first takes it below half,
it sheds one canister of 3 tonnes to get away. When it goes, by anyone's
hand, its whole load floats free: 10, 12, 14 or 16 tonnes by its size,
in 3 or 4 canisters, of the two goods this system's economy wants most
(`world::HAUL`), spread a few hundred metres across where it was and
drifting slowly. Shooting one still makes you WANTED.

**Raider TAGs and the bounty.** A raider pays nothing when it dies.
When your own shot downs it (the nose laser or the gunner's turret), it
drops a TAG, the proof of the kill (note RAIDER DOWN: SCOOP ITS TAG).
Raiders finished by a warden, and haulers and wardens, drop none. A TAG
is a canister in one colour that blinks mint (32 ticks on, 32 off,
about once a second) in the view and on the scanner; its bracket stays
magenta; the target line calls it RAIDER TAG and says FLY INTO IT. Fly into it
to scoop it (SCOOPED:
RAIDER TAG): it takes no hold space, so it is scooped even with a full
hold. Your laser passes through it (it takes no damage and never
shields a raider behind it, and the COCKPIT crosshair never locks on
it). Not
scooped, it is gone after 120 s (cargo after 90 s) and drifts like
cargo. Each TAG is worth 25 credits times the raider's gun level (1
to 3) plus half the system's danger (rounded down), at most 100: 25,
50 or 75 in SECURE and QUIET systems, 50, 75 or 100 in RISKY and
LAWLESS ones (`world::tag_worth`), fixed at the kill. You carry at most 255; then a further
TAG stays in space. The station's bounty office pays all TAGs when you
dock (BOUNTY PAID: 225 CR), before the charter's sum is checked, so a
bounty can win the charter at that dock. A LAWLESS station has no
bounty office (NO BOUNTY OFFICE HERE): you keep the TAGs, and the next
lawful station pays them (one switch, `world::LAWLESS_PAYS`). An OUTLAW
is paid nothing (OUTLAW: NO BOUNTY PAID) and keeps the TAGs too; the
warrant stays behind in that system.

**Fights on 160 x 120.** Each ship kind has its own outline and colour:
the raider a red three-pointed claw, the hauler a gold block with a
glass cab, the warden a white and blue lozenge with a tall fin. Ships
are drawn in true perspective (a ship twice as far is half as big),
far ones as a 2x2 dot of their kind's colour, have an engine glow seen from
behind, flash mint (the colour of your bolt) when hit, and burst in stages (flash, fireball,
sparks, fragments, smoke). Laser bolts are short and two pixels thick and fly through space, so a
shot at you ends as a streak from the shooter down to your hull.

**Raiders (step M1).** A raider's gun reaches as far as your laser
(2.4 km) and has one of three levels (`world::RAIDER_GUNS`: damage a
hit, ticks between shots, hit chance): level 1 does 5 a hit, level 2
8, level 3 16. The level comes with the system's danger
(`world::GUN_ODDS`, picked by the ship's look): SECURE all level 1,
QUIET half 1 and half 2, RISKY mostly 2 and one in four 3, LAWLESS
mostly 3 and one in four 2. Its hit chance is whole within 1.2 km and
two thirds at 2.4 km, and your turning spoils it: your pitch and yaw
rates above a small dead zone (`STEADY`, the small turns that keep a
foe in the sights) are averaged over about half a second
(`Me::jink`), and the chance is multiplied by 48 / (48 + that
average), so a pilot who jinks now and then while closing is hit
about a third as often as one who flies straight. A raider flees only
after it has fired twice and is below 30 % of its hull; it then runs
flat out (5/4 of its top speed), weaving, for 10 s, and comes back to
the hunt unless you are within 3 km of it (chased, it keeps running
and is gone at 14 km). Packs grow with the danger (`world::PACKS`): one
raider in SECURE systems (the start system among them), 1-3 in QUIET,
2-4 in RISKY, 3-5 in LAWLESS. A pack of four or more comes as two
groups, three in front and the rest 3 km behind them. There is no
limit on how many attack you at once, but no new pack comes while a
raider still presses an attack within 9 km (one that flees does not
count), so packs come one after another and never pile up. Wardens
and haulers are unchanged.

**Sizes (step M2).** Other ships are drawn and hit at their true size
in true perspective at every distance (before, from 600 m out to 7 km
a ship was drawn as if 600 m away). Fighters (raiders, wardens) are
three times their size before (`world::FIGHTER_X`), freighters twice
the fighters (`world::FREIGHTER_X` = 6): a raider is about 260 m
across, the biggest hauler about 340 x 270 x 800 m, the biggest
tanker about 440 x 180 x 870 m. Rocks and canisters are twice their
size before (`world::SMALL_X`: rocks about 150 to 235 m, a canister
30 m, scooped within 70 m). The station is four times its size before
(`world::STATION_X`: about 2.15 km across the wings, the hub 800 m long
and 400 m in radius) and its door is 450 x 280 m, so the biggest
freighter fits through it; the tow lane, the auto-dock, departures
and arrivals moved out with it. Your own ship is drawn `world::ME_X`
(6) times its size before since step F1: the LARK about 250 m across
and 190 m long, four fifths of a raider's length (step M5's DRAY and
SHRIKE are in the same size class: their model balls are 24 and 23 m
against the LARK's 25 m, all drawn at the same `ME_X`). The chase eye sits 420 m
behind and 96 m above it (so it looks as before), the launch point is
990 m out, the station's hub keeps 120 m (`world::ME_R`) off your
ship's middle, and the cockpit view is unchanged. A ship whose ball is
under 3 pixels on the screen is a 2x2 dot of its kind's colour, and
under 6 pixels the dot is drawn under the model too, so a thin hull seen edge-on never
vanishes. Hit sizes, aim help (now 1/40 of the distance), formation
and pack spacing, break-off distances, sparks, debris and trails grew
with the ships. A raider's and a warden's fire reach did not. Since
step F2 a raider's hit size is 72 m, smaller than its drawn half span
(about 130 m) but well over its body, so shots land about as often as
before step M2, and the auto-dock flies in at a speed that falls with
the way left (1/1000 of it, from full speed 5 km out down to 300 from
1.2 km in), so the time round the station is about as before M2. Building
with `RUSTFLAGS='--cfg merc_sizes_b'` gives sizes B for comparison
(fighters twice, freighters 2.5 times the fighters); the normal build
is sizes A.

**Space dust in the chase view (2026-11-23).** The space dust is what
shows your own speed: specks fixed in space on a 64 m grid, drawn as
short streaks along the way the ship goes (`scene::dust`, at MID and
HIGH). Since step F1 the chase view had put the grid round the ship,
420 m in front of the eye, with cells and streaks `ME_X` times bigger:
it showed about twice the cockpit's specks, but each crossed the
picture at about a sixth of the cockpit's pace, so the chase view felt
slower. Now the grid is round the eye in both views, at the
cockpit's size: the specks rush past the chase eye as they rush past
the cockpit. The cockpit view's dust is unchanged, and so are the star
streaks of a jump, cruise and the overboost (the same in both views).
Measured on the PC by `tests/speed.rs` (2026-11-23, the same flight
seen from both eyes, chase / cockpit): at HIGH about 5.5 specks in the
view in both, at top speed 2.25 / 2.20 pixels a step and streaks of
8.8 / 8.5 pixels; at MID 2.4 to 2.5 specks, 2.60 / 2.55 pixels a step
and streaks of 11.0 / 10.5 pixels. Before the change the chase view had
11.8 (HIGH) and 5.5 (MID) specks at top speed, moving 0.35 and 0.41
pixels a step. The chase view now draws as many dust lines as the
cockpit (before, about twice as many).

**Smoke, bolt colours and the live dock (step M3).** A ship under half
its hull trails light smoke, under 30 % heavy smoke with flickers of
fire (`fx::SMOKE_HALF`, `SMOKE_HEAVY`); your own ship too in the chase
view, and in the cockpit view wisps rise over the lower part of the
view. Smoke is drawn from the tick alone (no state, no dice). Every
bolt shows its gun level, for you and for raiders alike: level 0 sky
blue, 1 white, 2 red (`scene::BEAM_LV`); mint stays the TAG's blink
and the hit flash. While docked the scene behind the menus lives on:
the station turns, haulers pass on fixed lanes, a warden circles in
SECURE and QUIET systems and the camera sways slowly
(`World::scenery`, `scene::DOCK_SPIN`, `DOCK_TURN`, `DOCK_SWAY`).
MARKET and OUTFIT are darkened panels over it (drawn at LOW detail
there). Only looks move: no rule time passes while docked (prices, the
TAG's life, raider packs, hull and cargo stay as they were), the
scenery ships are cleared at launch, and no raider comes near. The
game draws 30 pictures a second there instead of resting, so the
console no longer rests while docked (more processor work and power,
no memory change); `fx::DOCK_LIVE_ON = false` gives the still docked
screens back.

**Overboost (step M4).** Where cruise is blocked (within 3 km of the
station, near the planet or the sun, or while a raider hunts you
within 6 km), press X at top speed and keep it held: the ship pushes
on to 7/4 of top speed (the LARK: 525 m/s instead of 300,
`world::OD_TOP`, reached in a third of a second; each ship has its own,
see step M5), for as long as you hold X and the charge lasts. A full
charge (`OD_FULL`) gives the LARK 3 s; it fills again in
9 s whenever the overboost is off (in cruise too), and it needs a
quarter (3/4 s, `OD_MIN`) to start, else the press only shows NO
CRUISE as before. Meanwhile the stick turns the ship half as fast, the
nose laser and the gunner's turret cannot fire, the COCKPIT view's
crosshair is red and does not lock, the view's top line says
OVERBOOST (in both views), and stars
streak past. Letting go of X, cruise or the auto-dock ends it at once;
afterwards the speed falls back to top within half a second. The
raiders' aim suffers as if you turned hard all the time (a third of
its usual chance, `OD_JINK`). Where cruise is allowed the same press
gives cruise, as before. The charge line under SPD is amber while it
runs, green when it can start, dim below that. Level-3 raiders
overboost too, to 7/4 of their own top speed, never firing meanwhile:
the first 3 s of a flight (so a hurt one gets away faster), and up to
3 s when the prey it hunts gets out of its gun's reach, ending at once
when the prey is back in reach, at most once every 12 s (`OD_RUN`,
`OD_EVERY`; `RAIDER_OD_ON = false` turns that off). Levels 1 and 2
never do.

**Ships to buy (step M5).** Three ships, each an original model of
its own outline in the chase view (`world::CRAFTS`, `models::CRAFT_MODELS`):

| Ship | Price | Tech | Hold | Top speed | Turn (yaw, of the LARK's) | Hull | Shield | Overboost |
|---|---|---|---|---|---|---|---|---|
| LARK (the courier you start in) | 1,600 | 1 | 24 t | 300 m/s | 100 % | 100 | 100 % | 3 s, 525 m/s |
| DRAY (cargo) | 3,600 | 4 | 60 t | 225 m/s | 68 % | 70 | 100 % | 2 s, 337 m/s |
| SHRIKE (combat) | 9,000 | 7 | 12 t | 360 m/s | 118 % | 160 | 125 % | 4 s, 630 m/s |

A station sells the ships up to its tech level. Buying one trades in
the ship you fly: it is worth 3/4 of its list price (`world::TRADE_IN`)
less the cost of mending its hull (2 credits a point), and when that
is more than the new ship's price the station pays you the difference.
The laser, the shield, the sun skimmer, the fuel, the cargo and the
TAGs move over; the new ship's hull, shield and overboost charge are
full. The cargo must fit the new hold (else SELL CARGO FIRST). The
CARGO BAY 40T is gone from the outfit; the DRAY takes its place. The
shield's strength is the fitted shield level times the ship's share
(`Craft::shield`), the hull bar and the speed bar are measured against
the ship's own most. The DRAY is drawn with a wide glowing engine framed
in a box, the SHRIKE as a dart with four fins in an X, the LARK as
before.

When the ship is lost (CARGO AND TAGS LOST), A goes back to the last
dock as it was then, but with the hold empty and no TAGs
(`Career::wrecked`): money, fuel, gear, hull, surveys and the day are
as at that dock (the ship too). That dock is then the empty-holded one, so a second
loss before the next dock finds the hold still empty. B goes to the
title.

## More life (steps L1 to L4)

Four steps added life to the flight on 2026-11-22. Each part has its
own switch (a `bool` constant, `true` today), so it can be turned off
to free time or bytes without touching the rest.

- **L1, effects** (`src/fx.rs`: `SPARKS_ON`, `DEBRIS_ON`, `SHAKE_ON`,
  `STREAKS_ON`, `NEBULA_ON`, `CORONA_ON`, `FLARE_ON`, `AIR_ON`,
  `LIGHTS_ON`, `GUIDE_ON`, `TRAILS_ON`, `SHOCK_ON`): engine trails,
  laser sparks, wrecks that burst with a shock ring and tumbling pieces,
  a shaking view and a shield ripple when you are hit, warp streaks
  after a jump and in cruise (the same in both views; the space dust
  that shows your own speed is described under "Space dust in the
  chase view"), a nebula seeded by the system, the sun's
  corona, turning rays and lens rings, a planet air rim, and the
  station's lights (wing tips, a strobe, running guide lights).
- **L2, traffic** (`TRAFFIC_ON`, `WINGS_ON` in `src/world.rs`): the
  ship slots went from 8 to 12 (`SHIPS`); fights still use the first 8
  (`FIGHT`), the last four hold only traffic, rolled from the tick so
  the fights' dice are unchanged. Ships leave and arrive at the
  station, a warden patrols SECURE and QUIET systems, far convoys fly
  with escorts (or with raiders in RISKY and LAWLESS systems), and
  raider packs fly as a V until they are 2 km from their prey.
- **L3, hulls and looks** (`HULLS_ON`, `MARKS_ON` in `src/scene.rs`;
  `LIVERY_ON` in `src/art.rs`): three more original hulls, TANKER (a
  hauler, 20 faces), HOOK (a raider, 16) and CUTTER (a warden, 14);
  each system flies the second hull for a quarter, half or three
  quarters of its ships, ships vary in size and carry one of four
  markings, and each system has its own trader and pirate colours. The
  drawing only reads this; the hit sizes are unchanged.
- **L4, rocks** (`ROCKS_ON` in `src/world.rs`): far from the station,
  now and then up to three rocks (ICE or ORE; model `ROCK`, 9 corners
  and 14 faces) drift about 3 km ahead. A rock has 24 hull points;
  shot apart it bursts and leaves one canister of 1 to 3 tonnes of its
  good. Breaking a rock is no crime, rocks are never scooped whole, and
  the target line says ROCK. A rock leaves after 90 s.

Two rules changed with them: traffic ships (the last four slots) leave
when they are 12 km from you, while fights keep 22 km; and ORE pays
about 14 credits a tonne at base price, ICE about 6.

On the board (MEASURED ON THE BOARD, the builders' pad scripts played
in parts, step time of the game's own work, 2026-11-22):

- L1 in firmware 1122n: 3.8 to 7.4 ms on average per part, one single
  step of 14.1 ms while turning past the planet and the station; 60
  pictures a second in flight.
- L2 in the owner's test firmware 1122o-own: one single step of 16.3 ms
  in the same place (the picture has 16.7 ms at 60 a second); what it
  was is NOT CHECKED. The pictures a second did not drop.
- L3 and L4 together in 1122p-own: part averages 4.32 to 5.64 ms, the
  worst single step 7.86 ms; 59.99 pictures a second in flight (36.02
  in the part with the menus). That run did not pass the station again,
  so the 16.3 ms step was neither repeated nor cleared.
- The starting settings of 2026-11-23 (DETAIL HIGH, VIEW CHASE, 60) in
  the owner's firmware of that day, keys sent over the debug link, no
  gamepad connected, the console's whole step: about 20 s of flight
  just after launch at 59.99 pictures a second, 4.51 ms a step on
  average and 5.21 ms at most; about 20 s more after 5 s of thrust at
  59.99, 4.43 and 4.80 ms; 0 crossed windows in both. A fight and the planet were not
  in these runs.

No person has played L1 to L4 yet; how they feel and whether the
balance still holds in play is NOT CHECKED (only `tests/balance.rs`).

## The planet (steps G1 to G4)

A system's planet has one landing site (none on the gas giant). You
fly down to it, set the ship down by hand, walk out, cut the site's
seams and the wreck for goods, fight the drones that guard the seams,
and lift off again before the weather closes in. The code is in
`src/planet/`; every number below is in `src/planet/tune.rs`, with the
spec's value beside it where the spec gives one (labels: READ FROM THE
CODE unless said otherwise). Nobody has played it on the board yet.

**Getting there.** Docked, the station's screen shows the site under
the name (`SITE  ORE 4  WRECK  DRONES 3`: the seams' good by the look,
GRAIN, ICE or ORE, how many seams, the wreck, how many drones;
`SITE  CUT TODAY` once you took it, `SITE  NO SURFACE` on a gas giant).
In flight a ring of eight amber lights hangs 1.5 km over the site, 600 m
across (300 m from its middle), with a green SITE beacon. Fly through it nose down (45 degrees
or steeper) at no more than top speed, without cruise or overboost,
with 0.3 LY of fuel. A raider hunting you within 3 km closes the ring
(RING CLOSED: RAIDERS NEAR), so the planet is no escape from a fight.

**The way down.** A 3 s entry (A skips it after 1 s), then you fly the
ship from 120 m up, 150 m from the pad, in a chase view 24 m behind.
Left stick pushes the ship sideways; the assist (on, SINK) brings it
down at a fifth of the height a second; Y switches it OFF and back;
a tap on the JETS box holds the height (HOLD); R, the right trigger or
the right stick up add lift. The jets heat up (HOT cuts them until
60 %). A shadow under the ship helps judge the height. The touchdown:
GREASED IT (1 m/s down and sideways or less, +20 s on the clock),
LANDED (4 and 3 m/s), HARD (up to 8 and 6 m/s, hull damage), CRASH or
TIPPED OVER on ground steeper than 12 degrees. Hull 0 is a loss.

**On foot** (after 1 s the hatch opens and the pilot steps out):

| control | what it does |
|---|---|
| left stick | walk (4 m/s), in the camera's frame |
| A | jump; held in the air, the hop jets (JET bar) |
| X held | the cutter: on the nearest seam, wreck, boulder or drone within 30 m and 25 degrees ahead |
| Y | the scan: a ring grows to 60 m and marks seams (amber), the wreck (white), drones (red) and boulders (grey) for 10 s; costs 20 of the jet energy |
| L / R, both | turn the camera 45 degrees; behind the pilot |
| right stick | turn and tilt the camera |
| B held 0.5 s at the hatch | board and lift off |

The panel shows SUIT, JET and CUT (the cutter's heat) bars, HOLD n/size,
TIME, a map, and the notes.

**Seams, the wreck, boulders.** A seam cracks in 3 stages of beam time
(0.8 / 0.6 / 0.45 s by laser level) and bursts into chunks, 3 / 3 / 4 /
4 t by danger (SECURE, QUIET, RISKY, LAWLESS; the spec has 2 + danger).
Walk over a chunk to take 1 t. The wreck gives 2 t of SPARES. A boulder
cut at its base rolls downhill; it knocks the pilot (30 suit points) and
crushes a drone it meets.

**Drones.** 2 + danger of them, at most 4, each circling a seam 3 m up.
One sees the pilot within 25 m and 60 degrees of its facing, or within
10 m any way round. Then it stops and aims for 1 s: a red line from its
eye follows the pilot, and for the last 0.15 s it stops following and
flashes white (locked). Then a bolt flies at 30 m/s to where it locked.
A side step or a jump in that last instant takes the pilot out of its
way; standing still is always hit. A bolt costs 8 + 4 x danger suit
points; a drone has 40 + 10 x danger points, and the cutter takes 30 /
45 / 70 a second by laser level. A downed drone bursts and drops 1 t of
SPARES, and stays down that day. Drones never follow past 40 m from
their seam.

**The clock and the storm.** TIME runs from the touchdown: 240 / 225 /
210 / 195 s by danger (the spec has 240 / 210 / 180 / 150; the drones'
fights take time). 30 s before the end a note names the weather (RAIN
SQUALL, DUST STORM, WHITEOUT, ASH FALL IN 30 S); at the end STORM: BACK
TO THE SHIP (or STORM: LIFT OFF aboard). The fog closes to 24 m and the
suit loses 3 points a second away from the hatch.

**The suit and losses.** The suit has 80 / 100 / 130 points by shield
level and refills at the hatch. Falls over 12 m/s cost points. At 0,
PILOT DOWN: the same loss as a ship lost in space (the cargo and unpaid
TAGs are lost; A goes back to the last dock as it was). What was cut and
the drones downed stay so until the day moves on (a jump).

**What a landing pays.** The goods in the hold, sold at a station. A
scripted pilot (it measures the rules, not how hard it is for a person)
on 100 real sites in a LARK with laser and shield level 0 brought home
on average 316 CR at SECURE, 321 QUIET, 390 RISKY and 378 LAWLESS,
valued at the galaxy's best price (MEASURED ON THE PC, `tests/foot.rs`,
2026-11-23, step G4); a trade jump pays about 530. LAWLESS is held back
by the LARK's 24 t hold.

**On screen.** Props (plants and small rocks, one try per 8 m cell, by
the look) drawn to 24 / 48 / 72 m by detail level; plants lie down for
3 s under the ship's and the pilot's jets. Sparks come from one pool of
64 (footsteps, the cutter's chips, the jets' scorch, a drone's burst,
a bolt's end, a boulder's dust); a full pool reuses its oldest slot. The
wreck sends up a smoke column until it is stripped. Drones, props and
plants are not drawn or moved while the ship is in the air, which keeps
the high views as cheap as before G4.

**Sizes and placement.** All of the planet's code and tables are in the
flash part (`cold` lines in `flash.list`), except its inner loops, which
run many times a picture and live in RAM (`ram` lines: the land's walk,
the ground's height, one thing's draw, the props' cell checks, the spark
pool's add and draw). The planet state `Below` is under its 2,048-byte
budget (`the_planet_state_fits_its_budget`). Sizes in "Chip build" below.

## Tests (from this folder)

```bash
cargo test --offline --release
```

- `galaxy`: the same seed always gives the same galaxy; 1000 seeds
  all give sane galaxies where every system can be reached and no two
  systems share a name.
- `play`: a scripted trade run makes money; the safe routes are safe in
  8 galaxies and 6 seasons (960 trips, no ship lost); scripted fights
  end; surveys pay once; the charter needs both the sum and the
  surveys; the gunner's turret downs a raider behind the ship and
  shares the heat; the tow lane takes a slow ship in once and docks it;
  a lost ship loses its cargo and TAGs and keeps the rest, a second
  loss without docking finds the hold still empty, and a broke pilot
  after a loss gets the free tank and earns a survey fee; a raider you
  down (laser or turret) drops one TAG and pays nothing yet, a warden's
  kill and hauler or warden kills drop none, the TAG takes the dead
  raider's slot when every slot is full, is scooped with a full hold,
  takes no damage, is never hit by the laser and does not shield a
  raider behind it, blinks and is gone after 120 s while cargo still
  goes after 90 s, and stops at 255;
  TAGs are paid on docking with the sum, kept unpaid at a LAWLESS
  station and by an OUTLAW and paid at the next lawful one, can win the
  charter at that dock, and are lost with the ship (everything else as
  at the last dock); the notes say so; the 52-byte save (`src/save.rs`,
  format 5 since step M5, with the ship) round-trips and refuses damaged
  bytes, other versions at its size (3, 4 and 6), an unknown ship, the
  old CARGO BAY bit, a hull or hold above the ship's, and TAG sums under 25 or over 100 each or not a
  multiple of 25; a format-4 save loads as the LARK, its CARGO BAY
  refunded at the bay's price and cargo over 24 t sold at that
  station's price.
- `hold_range`: holding a market key buys the same amount in the same
  game time at 30 and at 60 pictures a second; one press trades once;
  a hold stops at a full hold, no money, no stock and an empty hold
  with one note; it never moves to another row or screen (keys, stick
  and finger). In flight, the in-range box, the crosshair's lock and
  the target line agree with a copy of the laser's rule and with real
  shots, for ships just inside and just outside 2.4 km, off the aim
  line, behind, with a hot laser, and 600 random places; the distance
  text for known places.
- `hold_range_check`: the checker's attacks on the same round. Random
  held keys, sticks and fingers at both picture rates never trade
  without a held input, never trade off the market or on another row,
  never raise credits plus cargo value, and keep cargo within the hold
  and stock plus cargo constant; one note per held stretch; buy and
  sell held together repeat nothing; a counter near its limit cannot
  overflow. In flight: ships in line (the nearest is named, locked and
  hit), the station in the way (the laser passes through it, and the
  lock says so), the chase view, and the distance text from 0 m to
  250 km.
- `look`: writes a picture of every screen to `.local/mercator/shots/` (also the
  three ship kinds at 1.5 km, the blast's stages, the tow lane, the
  gunner, the station beacon, the market of a SECURE and a
  LAWLESS FORGE side by side, the range marks with a raider out of
  reach, in reach off the aim line and on it, a named hauler, the
  market while A is held, and the raiders' TAGs: the kill note, a TAG
  in flight, the panel's count, the bounty note, a LAWLESS station and
  the dock screen with TAGs unpaid (the panel count, the bounty sum,
  the dock line and the lost card checked pixel for pixel, the other
  notes as text);
  a TAG at 100 m, 300 m, 2 km and 6.5 km with its blink on and off,
  which must change the view and the scanner; the panel's TAG count
  at every value 1 to 255 with AUTO lit and dim, which must keep at
  least 5 empty columns after AUTO, end at x 157 and change nothing
  else; a TAG's target line in and out of laser reach, which must end
  in FLY INTO IT and never say IN RANGE while a raider and a cargo
  canister in reach keep IN RANGE, and which must fit and keep off the
  corner beacons at the longest distance text and in play; a TAG's
  line, bar and corners in magenta near and far, blink on and off, with
  nothing amber on the view, while a hunting raider keeps its amber box,
  line and bar in reach and its red corners and dim line out of reach),
  measures the heaviest scenes
  (triangles, painted pixels; since 2026-11-23 the flight scenes in
  both views, COCKPIT and CHASE) and what each picture sends to the
  screen (`console_video::diff`), and checks that the same keys give
  the same game.
- `market`: the risk rule in the prices. SECURE prices are exactly the
  old rule's (every system of the 10 galaxies, 203 days); each danger
  step only makes made goods cheaper and wanted goods dearer; a
  station never pays back what it sells for, at any danger; no good
  pays well both ways between neighbours.
- `balance`: does risk pay? Four scripted pilots fly whole careers
  to 50,000 credits in every galaxy. Three traders: SECURE only,
  SECURE and QUIET, and all four danger levels; since step M5 they
  buy the cargo ship (DRAY) as soon as they can keep 1,100 credits
  after it, then the lasers. The fourth, the hunter (step M5), trades
  in SECURE and QUIET space until it can buy the combat ship (SHRIKE),
  then jumps to the best RISKY system in reach (lawful: its station
  pays for TAGs), waits 3 minutes at the arrival point for raiders,
  fights them and fetches their TAGs before it docks. Prints trips, days, minutes flown, ships
  lost, the TAGs (dropped, scooped, paid, lost at a death) and the
  trip a ship was bought on; best with `--release` (a debug build
  flies 3 galaxies unless `BALANCE_GALAXIES` says more). After a fight
  the pilots fetch TAGs within 3 km (at most two minutes a trip);
  bold, carrying 2,500 credits or more of TAGs at a LAWLESS station,
  keeps the next trip to danger 2 or less to cash them in
  (`BALANCE_CASH_AT`, 0 turns this off). MEASURED ON THE PC
  (2026-11-23, step M5, all 10 galaxies): mean jumps to 50,000
  credits secure 94.4, careful 87.8, bold 36.4 (45 ships lost),
  hunter 79.6 (9 lost, TAGs paid 25,522 credits a galaxy); after step
  J1 (with the bay, no hunter) 100.4, 93.2 and 33.0 (27 lost). The
  test fails when bold is slower than careful in more than one galaxy
  in five, or its mean is not under half of careful's, when any pilot
  misses the goal in a galaxy, when the hunter's TAGs pay less than
  8,000 credits a galaxy or it downs fewer raiders than careful, and
  checks that every scooped TAG was paid, lost or still carried.
  Scripted pilots measure rules, not how hard the game is for a person.
- `risk_check`: the risk rule checked again in wide numbers (every
  system, good and danger; `RISK_ALL_DAYS=1` for all 65,536 days, best
  with `--release`); same-good loops and the best profit a jump between
  neighbours, old rule against new; a market-only trader per galaxy at
  several strengths; each economy pair's profit at danger 0/0 and 3/3.
- `attack`: hostile inputs (random saves, taps everywhere, the world's
  edges) never break the game.
- `fx`, `traffic`, `hulls` and `rocks`: the steps L1 to L4. Among them,
  `rocks` (3 tests) checks that a rock field drifts ahead and is never
  scooped, that a rock shot apart leaves ORE or ICE and is no crime, and
  that rocks and convoys both come (10 simulated minutes far out gave
  16 rocks and 3 convoys).
- `raiders` (7 tests and one measuring run): step M1. A raider fires
  back from about 2.4 km; a level-3 raider costs a pilot who flies
  straight at it and fires at least 35 points, a weaving one at most
  3/4 of that; it flees only after trading fire, runs, and comes back
  unless chased; pack sizes follow `PACKS` and the start system sends
  one; no pack comes while one hunts; gun levels follow `GUN_ODDS` and
  the TAG follows the gun. `cargo test --release --test raiders --
  --ignored --nocapture` prints duel, patrol and picture-cost tables
  (`RAIDER_TABLES=duel`, `patrol` or `cost` for one). Scripted pilots
  measure rules, not how hard the game is for a person.
- `boost` (16 tests and one measuring run): step M4. The overboost
  starts only with a held X press where cruise is refused and with a
  quarter charge, lasts exactly 3 s from full, never passes 7/4 of top
  speed, refills in 9 s, stops on release, cruise or the auto-dock,
  is full again at a launch and a jump, halves the turn rate, keeps
  the laser and the turret silent and the crosshair unlocked, and cuts
  the raiders' hits to a third; level-3 raiders overboost 3 s out of
  reach (at most every 12 s, ending once back in reach) and when they
  flee, never firing then, levels 1 and 2 never; the same presses give the same flight; the X
  key and the faster bars hold it; the panel's charge line and the
  red crosshair. `M4_SHOTS=<folder>` writes its pictures; `cargo test
  --release --test boost -- --ignored --nocapture` prints the escape,
  chase and head-on tables with and without it.
- `dock_live` (4 tests): step M3. Half a minute on each docked screen
  changes no rule value, and the flight after it is the same as after
  an immediate launch; the docked scene moves, the pause rests, the
  same keys give the same pictures; pictures `50-dock-*` to
  `53-dock-paused-*`. `fx` holds M3's smoke and bolt pictures
  (`49-fx-*`) and counts the heaviest fight's particles; `look`'s cost
  table has the live docked screens (since step M5 the SHIPYARD too)
  beside the heaviest flight scenes.
- `ships` (8 tests): step M5. The ship table holds the numbers of the
  table above (checked against the test's own copy);
  each ship flies at its own top speed, turn rates, shield and
  overboost; the trade-in is 3/4 of the list price less the mending;
  every refusal (not sold here, the ship you fly, too little money,
  cargo that does not fit) and the credit cap; the SHIPYARD buys by
  keys and by two taps (pictures `M5-shipyard-*`, `M5-docked-menu`);
  the three ships differ in the chase view in space and at the
  station (pictures `M5-chase-*`, at least 150 pixels apart); no ship
  makes the chase view or the SHIPYARD too heavy.

- `sizes` (step M2), `f1` and `f2` (steps F1 and F2): the sizes and
  perspective, the big own ship and the haulers, the auto-dock's way
  in and the raider hit size; `f2_measure` (ignored) prints F2's
  measures. `golden`: the game driven through its `game_entry` with a
  fixed script; every picture's hash and every log line are compared
  with `tests/data/golden.txt` (`UPDATE_GOLDEN=1` renews it after a
  deliberate change of look). Since 2026-11-23 the script flies first
  in the starting settings (CHASE, HIGH), then the pause menu turns
  them to COCKPIT and MID (mark `cockpit-mid`).
- `speed` (2026-11-23): the sense of speed in the two views. The same
  straight flight, askew to the dust's grid, seen from the cockpit eye
  and the chase eye at MID and HIGH, still and at 75, 150, 300 and
  525 m/s and in cruise at 3 and 15 km/s: the specks in the view, how
  far a speck moves from one step to the next, and its streak's length
  must each stay within 3/4 to 4/3 of the cockpit's in the chase view;
  still, nothing moves or streaks. `cargo test --release --test speed
  -- --nocapture` prints the table.
- `fuzz`: random keys, and a random pilot, for game hours. For long
  runs: `FUZZ_MINUTES=6000 FUZZ_HOURS=1000`, best with overflow checks
  and debug assertions on in the release profile.

All together: 211 tests and 6 ignored measuring runs (MEASURED ON THE
PC, 2026-11-23, after step G4); 212 with `speed` (MEASURED ON THE PC,
2026-11-23, after the chase view's dust change). `drones` (step G4, 15 tests) tests the
drones' telegraph (1 s, the last 0.15 s locked), that a side step or a
jump in the last instant escapes the bolt and standing still never does,
a bolt's damage by danger and suit level, the cutter and a rolling
boulder downing a drone, the 40 m leash, the spark pool that never runs
over, the props (kept off the things, lying down under jets), the scan's
marks, the heaviest picture's budget at the three detail levels, the
storm note, and the cost of every planet scene by detail level (with
pictures). `foot` (step G3, 15 tests) tests the
visit out of the ship: the walk stays on the site and never sinks,
falls and the hop jets, the cutter's stages and heat, chunks that keep
their tonnes and never overfill the hold, boulders that roll and stop
and knock the pilot, the clock and storm by danger, the suit's drain
and refill, the take-off's write-back, PILOT DOWN as a loss, a cut site
that stays cut until the next day, the cost of a picture on foot, a
scripted visit on 100 sites (since step G4 with the drones, and
checked against trading: a SECURE visit pays at least half a trade
jump, and RISKY and LAWLESS pay more than SECURE and QUIET) and pictures
on foot. `landing` (step G2) tests the way down:
the touchdown table's edges, the assist, lift by ship and load, the
heat, the take-off, a loss below, and a scripted pilot landing softly
on 100 seeds of every landable look. `site_test` runs only in the
measuring build with `RUSTFLAGS='--cfg merc_site_test'` (LAUNCH then
starts 1 km above the landing site's ring; never for use). No check
script builds that cfg or `merc_sizes_b`. The spec the planet's
sources name (SPEC 3.x) and the step notes they cite (BOARD-G2.md and
the like) are working notes kept outside the repository; every number
they set is restated in `src/planet/tune.rs`.

## Chip build

`games/build-chip.sh <this folder>` builds `MERCATOR.BIN`. With L1 to
L4 the code no longer fits the 57,344-byte code slot and runs on into
the top of the data slot (the spill, see `games/README.md`); since
step J1 part of it is in the flash part (next section). MEASURED ON
THE PC with `build-chip.sh`, 2026-11-23, after step G4 (the planet's
slice 1 finished: drones, the scan, sparks, props, the wreck's smoke),
the PILOT DOWN card title and the closing fixes:

```text
MERCATOR code  76016 of 57344 (+18672 past the slot)   data  27740 (constants 18672, starting values 9064, zeroed 4)   room left 2740 of 106496
MERCATOR flash part: code  81076 of 81920   constants  25732 of 32768
```

After the starting settings (HIGH, CHASE) and the chase view's dust
change (MEASURED ON THE PC with `build-chip.sh`, 2026-11-23):

```text
MERCATOR code  75972 of 57344 (+18628 past the slot)   data  27708 (constants 18640, starting values 9064, zeroed 4)   room left 2816 of 106496
MERCATOR flash part: code  81076 of 81920   constants  25732 of 32768
```

The dust no longer scales by `ME_X` for the chase eye, which gave back
76 bytes of room; the flash part kept its sizes.

Step G4 itself (its build of the morning of 2026-11-23: flash code
81,256, room left 2,732) cost 2,508 bytes of room: the drones', props' and sparks'
per-item helpers that run many times a picture are in RAM by `ram`
lines (`planet::show` and `hidden`, one thing's draw and its line of
sight; `props::spot`, `keep`, `within`, `at_cell`; `sparks::add` and
`draw`; `planet::px`: 1,682 bytes), and `Below` grew from 1,136 to
1,960 bytes (the drones, the 64 sparks of 8 bytes, the 16 laid plants,
the scan). Everything else of G4 is in the flash part, which grew by
16,684 bytes of code and 4,472 of constants; G4 raised MERCATOR's
constants place from 24,576 to 32,768 bytes (`ro_max` 0x8000 in
`crates/game-api/src/file.rs`). The flash code had 664 bytes left of
its 81,920 then. Since then the end card's PILOT DOWN title added 20
bytes of flash code, and the closing fixes (five sums in
`src/planet` that can no longer overflow) took 200 bytes of flash code
and 96 of constants away: 844 bytes are left today, so the next planet
step still needs room found first. The restart's
stack frame grew to 9,104 bytes (G3: 8,288), `planet::draw`'s to 1,280.
After step G3: code 74,748, data 26,508, room 5,240; flash part code
64,572, constants 21,356 of 24,576.

Step G3 cost 9,588 bytes of room. Most of it is not the walk: the
board showed (BOARD-G2) that the planet's inner loops (the land's
walk over its triangles and corners, the ground's height) run at half
speed from flash, so 19 of them now live in RAM for good (`ram` lines
in `flash.list`, about 11,400 bytes), with the walk's own small
per-thing helpers (the things' line of sight, the chunks' and
boulders' roll, the cutter's aim, the map's dots). To pay for it the
docked panels, the market, the outfitter, the shipyard's panel, the
LOST or WON card, the jump tunnel and the title text moved to flash
(they run once a picture of their own screen, never in flight). The
walk, the cutter, the chunks and boulders, the clock and the on-foot
camera and panel are in the flash part, which grew by 15,860 bytes of
code and 7,060 of constants. After step G2: code 67,224, data 24,444,
room 14,828; flash part code 48,712, constants 14,296.

Step G2 cost 340 bytes of room (the ring's hint line in space, and
the console's noted presses handed below, so a tap shorter than a
picture still counts there): the entry, the lander, the camera, the
shadow and the landing panel are all in the flash part, which grew
by 13,208 bytes of code and 1,808 of constants. After step G1: code
67,132, data 24,196, room 15,168; flash part code 35,504, constants
12,488.

Step G1 cost 1,764 bytes of room: the planet's code and tables run
from the flash part, and what stays in RAM is the space side (the
ring's check and marks, the docked SITE line, about 1,100 bytes of
code) and the planet's state (`Below`, 236 bytes, and the site cached
in `Bodies`). Its biggest stack frame is the land's drawing, 2,976
bytes. Before it (after step M5): code 66,032, data 23,532, room
16,932; flash part code 14,348 of 32,768, constants 9,160 of 16,384
(the place was raised in G1, see `games/README.md`).

Step M5 cost 4,620 bytes of room (after step J1: code 62,940, data
22,004, room 21,552; flash part code 12,880, constants 9,048).
Without the flash part (after step M4) the code was 74,824 bytes, the
data 31,012 and the room 660. The biggest stack frames are the start
(`restart`) 7,136 bytes, `scene::draw` 896 and `update` 736. Before the
steps L1 to L4 the code was 56,480 bytes and the room 25,436, so any
new feature must pay for itself in bytes. Code-size habits that keep
it small: wrapping sums where the range is known (no overflow-check
code), text drawn and logged byte by byte (no `core::fmt` padding, no
UTF-8 check), and small helpers that many places call kept out of line
(`#[inline(never)]`).

## Flash part

Part of the game runs straight from the chip's flash instead of the
RAM slots, which frees room in them. `flash.list` in this folder says
which part (`games/chip/flash_split.py` reads it; `games/README.md`
has the rules).

- In RAM: everything that runs on every picture. That is the flight,
  a fight, the chart opened in flight, the live docked scene behind
  the menus, the title's turning ship, and the drawing, smoke, dust
  and overboost under them. These are the `hot` lines; the build
  keeps whatever they call in RAM too. Since step G3 the panels over
  the docked scene (docked menu, MARKET, OUTFIT, SHIPYARD), the title
  text, the jump tunnel and the LOST or WON card are `cold`: each runs
  once a picture of its own screen only, never in flight.
- In RAM although only the planet calls it (step G3, `ram` lines):
  the planet's inner loops, called many times a picture below (every
  triangle and corner of the land, the ground's height, the things'
  line of sight, the chunks' and boulders' roll). The board measured
  them at twice the time from flash (BOARD-G2).
- In flash: code that runs once when something happens. That is
  docking, launching, a jump, dying, a crash, rocks, payment, a
  survey, new ships and a hauler's canisters (`World::shed`), buying a
  ship (`World::buy_ship`, `App::shipyard_keys`, step M5), the
  debris of a blast, notices, what a key press does in a menu, the
  start of a game, loading and saving, and the panic stop, with the
  tables only they read. These are the `cold` lines.
- In flash although it runs on every picture: the planet below (step
  G1: `planet::begin`, `tick`, `draw`, `panel`, the lift-off, the
  palette below, and where the site is, worked out once on entering a
  system; step G2: the entry, the lander, the chase camera, the shadow
  and the landing panel; step G3: the walk, the cutter, the chunks
  and boulders, the site clock and storm, the on-foot camera and
  panel; step G4: the drones, the spark pool's tick, the laid plants,
  the scan, the smoke column and the drones' lines and bolts). This is
  by design (the planet is the flash part's main user); the planet's
  shared helpers (`fill_tri`, the renderer, the text) stay in RAM, and
  so do G4's per-thing, per-prop-cell and per-spark helpers (`ram`
  lines, see "Chip build").

The `MERCATOR.BIN` this builds is a game file with a flash part
("GAMF"). It runs only on a firmware built with the feature
`game-flash` that carries exactly this part: the `flash/MERCATOR/`
files (`CODE.BIN`, `RO.BIN`) from the same build. A firmware built
with any other part refuses the game ("FLASH PART MISSING" or "FLASH
PART DIFFERS"). After any change to the game, build the game and the
firmware again together. The simulator links the game directly and
has no flash part.
