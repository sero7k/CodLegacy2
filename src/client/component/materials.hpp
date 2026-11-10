#pragma once

#include "game/game.hpp"

namespace materials
{
	bool setup_material_image(game::Material* material, const std::string& data);
	// Only rows [frame * frame_height, +frame_height) of the picture (a flipbook frame).
	bool setup_material_image_frame(game::Material* material, const std::string& data, int frame, int frame_height);
	game::Material* create_material(const std::string& name);
	void free_material(game::Material* material);
}
