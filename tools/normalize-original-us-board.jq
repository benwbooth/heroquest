def rect($x; $y; $width; $height):
  {area: {x: $x, y: $y, width: $width, height: $height}};

def room($name; $x; $y; $width; $height; $tint):
  {name: $name, area: {x: $x, y: $y, width: $width, height: $height}, tint: $tint};

def canonical_corridors:
  [(rect(0; 0; 26; 19) + {tint: [0.22, 0.24, 0.25]})];

def canonical_rooms:
  [
    room("Northwest Guardroom"; 1; 1; 4; 3; [0.36, 0.20, 0.16]),
    room("Northwest Library"; 5; 1; 4; 3; [0.25, 0.31, 0.22]),
    room("Northwest Cellar"; 1; 4; 4; 5; [0.22, 0.29, 0.30]),
    room("Northwest Hall"; 5; 4; 4; 5; [0.31, 0.22, 0.17]),
    room("Northern Gallery"; 9; 1; 3; 5; [0.22, 0.30, 0.24]),
    room("Northern Shrine"; 14; 1; 3; 5; [0.42, 0.28, 0.16]),
    room("Northeast Forge"; 17; 1; 4; 4; [0.39, 0.19, 0.14]),
    room("Northeast Store"; 21; 1; 4; 4; [0.28, 0.31, 0.20]),
    room("Northeast Chapel"; 17; 5; 4; 4; [0.24, 0.25, 0.34]),
    room("Northeast Vault"; 21; 5; 4; 4; [0.34, 0.24, 0.14]),
    room("Central Crypt"; 10; 7; 6; 5; [0.29, 0.16, 0.22]),
    room("Southwest Barracks"; 1; 10; 4; 4; [0.33, 0.20, 0.16]),
    room("Southwest Armory"; 5; 10; 2; 3; [0.24, 0.30, 0.22]),
    room("Southwest Secret Room"; 7; 10; 2; 3; [0.28, 0.24, 0.18]),
    room("Southwest Tomb"; 1; 14; 4; 4; [0.24, 0.23, 0.31]),
    room("Southwest Treasury"; 5; 13; 4; 5; [0.39, 0.29, 0.14]),
    room("Southern Workshop"; 9; 13; 3; 5; [0.28, 0.23, 0.18]),
    room("Southern Prison"; 14; 13; 4; 5; [0.25, 0.28, 0.25]),
    (room("Southeast Hall"; 18; 10; 3; 4; [0.30, 0.19, 0.16])
      + {additional_areas: [{x: 17, y: 10, width: 1, height: 3}]}),
    room("Southeast Library"; 21; 10; 4; 4; [0.24, 0.30, 0.20]),
    room("Southeast Tomb"; 18; 14; 3; 4; [0.22, 0.24, 0.32]),
    room("Southeast Throne Room"; 21; 14; 4; 4; [0.37, 0.21, 0.18])
  ];

def rename_room($index; $name):
  .[$index].name = $name;

def quest_rooms($page):
  canonical_rooms
  | if $page == 11 then
      rename_room(20; "Starting Chamber A")
    elif $page == 13 then
      rename_room(12; "Stone Gallery B")
      | rename_room(20; "Armory A")
    elif $page == 14 then
      rename_room(11; "Witch Lord Tomb D")
      | rename_room(14; "Stair Chamber")
      | rename_room(15; "Hidden Library E")
    elif $page == 15 then
      rename_room(11; "West Guardroom")
      | rename_room(12; "Treasure Room B")
      | rename_room(13; "West Dining Room")
      | rename_room(14; "Southwest Barracks")
      | rename_room(15; "Southwest Hall")
      | rename_room(16; "Southern Gallery")
      | rename_room(17; "Southern Guardroom")
      | rename_room(18; "Southeast Antechamber")
      | rename_room(19; "Eastern Crypt")
      | rename_room(21; "Spirit Blade Shrine A")
    elif $page == 16 then
      rename_room(4; "Northern Crypt")
      | rename_room(6; "Northeast Crypt")
      | rename_room(7; "Witch Lord Throne Room")
      | rename_room(11; "Empty Tomb A")
      | rename_room(12; "Southwest Crypt")
      | rename_room(13; "Southwest Guardroom")
      | rename_room(14; "Stair Chamber")
      | rename_room(15; "Southwest Hall")
      | rename_room(16; "Southern Gallery")
      | rename_room(17; "Southern Guardroom")
      | rename_room(18; "Southeast Antechamber")
      | rename_room(19; "Eastern Crypt")
      | rename_room(21; "Southeast Hall")
    else . end;

.source.page as $page
| .corridors = canonical_corridors
| .rooms = quest_rooms($page)
| if $page == 3 then
    .events |= map(
      if .id == "B" then .trigger.room = "Southern Prison" else . end
    )
  elif $page == 9 then
    .events |= map(
      if .id == "C" then .trigger.room = "Southwest Treasury" else . end
    )
  elif $page == 11 then
    .events |= map(
      if .id == "C" then .trigger.room = "Southwest Secret Room" else . end
    )
  else . end
