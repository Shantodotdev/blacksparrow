# How Tide Pools Survive Low Tide

By Mara Quinn · March 14, 2026

Twice a day the ocean pulls back and leaves small pools of seawater stranded among the rocks. For the animals inside, the next six hours bring rising temperature, rising salinity and falling oxygen.

Yet tide pools are among the most crowded habitats on any coast. Anemones, sculpins, hermit crabs and periwinkles all make a living there, and each has its own way of riding out the gap between tides.

## Holding on to water

Many tide pool animals simply avoid drying out. Mussels and barnacles clamp their shells shut and trap a pocket of seawater inside. Aggregating anemones cover themselves with bits of shell and gravel, which shade their bodies and slow evaporation.

- Barnacles seal their plates with a tight lid called an operculum.
- Limpets press down on a home scar worn into the rock.
- Snails retreat into their shells and close a trapdoor.

## Coping with heat

Shallow pools can warm by ten degrees Celsius in an afternoon. Sculpins move to the deepest, shadiest corner, and some crabs climb out entirely to cool off in the breeze.

> The tide pool is not a gentle place; it is a daily experiment in endurance.

## Measuring a pool

Researchers log temperature every ten minutes with small waterproof sensors:

```python
for reading in logger.readings():
    print(reading.time, reading.celsius)
```

| Time | Temperature |
| --- | --- |
| 09:00 | 14 °C |
| 15:00 | 24 °C |

When the tide returns, the cold water resets every pool at once, and the cycle starts again.
