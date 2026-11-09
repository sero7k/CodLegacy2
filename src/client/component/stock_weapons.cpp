#include <std_include.hpp>
#include "loader/component_loader.hpp"

#include "console.hpp"
#include "stock_weapons.hpp"
#include "stock_weapons_data.hpp"

#include <utils/memory.hpp>
#include <utils/string.hpp>

#include <cctype>
#include <cstring>
#include <functional>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

namespace stock_weapons
{
	namespace
	{
		struct cached
		{
			game::StringTable* original;
			game::StringTable* result;
			int last_try;
			bool done;
		};

		std::mutex tables_mutex;
		std::unordered_map<std::string, cached> tables;
		utils::memory::allocator allocator;

		using hash_fn = unsigned int(*)(const char*);

		unsigned int hash_djb2(const char* s)
		{
			unsigned int h = 5381;
			for (; *s; ++s) h = h * 33 + static_cast<unsigned char>(std::tolower(static_cast<unsigned char>(*s)));
			return h;
		}

		unsigned int hash_fnv1a(const char* s)
		{
			unsigned int h = 0x811C9DC5u;
			for (; *s; ++s) { h ^= static_cast<unsigned char>(std::tolower(static_cast<unsigned char>(*s))); h *= 0x1000193u; }
			return h;
		}

		unsigned int hash_sdbm(const char* s)
		{
			unsigned int h = 0;
			for (; *s; ++s) h = static_cast<unsigned char>(std::tolower(static_cast<unsigned char>(*s))) + (h << 6) + (h << 16) - h;
			return h;
		}

		unsigned int hash_31(const char* s)
		{
			unsigned int h = 0;
			for (; *s; ++s) h = h * 31 + static_cast<unsigned char>(std::tolower(static_cast<unsigned char>(*s)));
			return h;
		}

		// The cell hash the engine uses is found by checking candidates against
		// the table's own cells; nothing is added when none of them matches.
		hash_fn find_hash(const game::StringTable* table)
		{
			const hash_fn candidates[] = { hash_djb2, hash_fnv1a, hash_sdbm, hash_31 };
			const auto cells = table->columnCount * table->rowCount;
			for (const auto fn : candidates)
			{
				auto checked = 0;
				auto ok = true;
				for (auto i = 0; i < cells && checked < 200; ++i)
				{
					const auto& cell = table->values[i];
					if (!cell.string || !cell.string[0])
					{
						continue;
					}
					++checked;
					if (static_cast<unsigned int>(cell.hash) != fn(cell.string))
					{
						ok = false;
						break;
					}
				}
				if (ok && checked > 0)
				{
					return fn;
				}
			}
			return nullptr;
		}

		bool weapon_loaded(const std::string& ref)
		{
			const auto name = ref + "_mp";
			return game::DB_XAssetExists(game::ASSET_TYPE_WEAPON, name.data()) != 0 &&
				!game::DB_IsXAssetDefault(game::ASSET_TYPE_WEAPON, name.data());
		}

		game::StringTable* build(game::StringTable* original, const std::string& name)
		{
			const auto is_stats = name == "mp/statstable.csv";
			if (is_stats && original->columnCount != column_count)
			{
				console::warn("stock weapons: %s has %i columns (need %i), not adding\n",
					name.data(), original->columnCount, column_count);
				return nullptr;
			}
			if (!is_stats && original->columnCount < 1)
			{
				return nullptr;
			}

			const auto hash = find_hash(original);
			if (!hash)
			{
				console::warn("stock weapons: cannot work out the cell hash of %s, not adding\n", name.data());
				return nullptr;
			}

			std::vector<int> add;
			std::vector<std::string> missing;
			for (auto i = 0; i < row_count; ++i)
			{
				const std::string ref = rows[i][4];
				if (weapon_loaded(ref))
				{
					add.push_back(i);
				}
				else
				{
					missing.push_back(ref);
				}
			}

			if (add.empty())
			{
				return nullptr;
			}

			const auto cols = original->columnCount;
			const auto old_rows = original->rowCount;
			const auto new_rows = old_rows + static_cast<int>(add.size());

			const auto result = allocator.allocate<game::StringTable>();
			*result = *original;
			result->rowCount = new_rows;
			result->values = allocator.allocate_array<game::StringTableCell>(static_cast<size_t>(cols) * new_rows);
			std::memcpy(result->values, original->values, sizeof(game::StringTableCell) * cols * old_rows);

			for (size_t n = 0; n < add.size(); ++n)
			{
				const auto& src = rows[add[n]];
				for (auto c = 0; c < cols; ++c)
				{
					const char* text = "";
					if (is_stats)
					{
						text = src[c];
					}
					else if (c == 0)
					{
						text = src[4]; // the weapon ref
					}
					else if (c == 1)
					{
						text = src[2]; // its class (weapon_assault, ...)
					}
					auto& cell = result->values[static_cast<size_t>(old_rows + n) * cols + c];
					cell.string = allocator.duplicate_string(text);
					cell.hash = static_cast<int>(hash(text));
				}
			}

			console::info("stock weapons: %s: added %i stock weapons (%i not loaded)\n",
				name.data(), static_cast<int>(add.size()), static_cast<int>(missing.size()));
			return result;
		}
	}

	game::StringTable* patch(game::StringTable* original)
	{
		if (!original || !original->name)
		{
			return original;
		}

		const std::string name = original->name;
		if (name != "mp/statstable.csv" && name != "mp/customweapons.csv")
		{
			return original;
		}

		std::lock_guard<std::mutex> lock(tables_mutex);
		auto& entry = tables[name];
		if (entry.original != original)
		{
			entry = {original, nullptr, 0, false};
		}
		if (entry.done)
		{
			return entry.result ? entry.result : original;
		}

		// (Weapon zones may not be loaded yet: try again later, not every call.)
		const auto now = game::Sys_Milliseconds();
		if (entry.last_try != 0 && now - entry.last_try < 2000)
		{
			return original;
		}
		entry.last_try = now;

		const auto result = build(original, name);
		if (result)
		{
			entry.result = result;
			entry.done = true;
			return result;
		}
		return original;
	}
}
