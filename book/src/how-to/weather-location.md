# How to set the weather location

Without a location, the weather widget uses the city of your time zone. To set one, add a
`weather` line to `~/.config/niri-dash/config.kdl`, as a place name:

```kdl
weather location="Hamburg"
```

or as coordinates, with the units if you want Fahrenheit:

```kdl
weather latitude=53.55 longitude=9.99 units="fahrenheit"
```

The data comes from Open-Meteo and needs no API key.

The weather widget also has `location` and `units` settings of its own, in its settings card in
edit mode: see [Time widgets](../reference/widgets/time.md#weather).
