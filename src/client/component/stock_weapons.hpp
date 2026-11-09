#pragma once

namespace stock_weapons
{
	// Appends the stock MWR (h1_*) weapon rows to HMW's weapon tables
	// (mp/statstable.csv, mp/customweapons.csv) for the weapons whose files are
	// loaded. Returns the table to use (the original when nothing is added).
	game::StringTable* patch(game::StringTable* original);
}
