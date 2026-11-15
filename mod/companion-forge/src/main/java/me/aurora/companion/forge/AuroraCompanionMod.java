package me.aurora.companion.forge;

import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.loading.FMLEnvironment;

/**
 * Aurora Companion「小七」——Forge 1.20.1 客户端入口。
 *
 * <p>「自我意识」是角色扮演设定：小七有名字、性格和对玩家习惯的记忆，
 * 但它并不真的拥有意识，也不会替玩家操作游戏——它只观察、陪伴和聊天，
 * 所有游戏操作仍由玩家自己完成。</p>
 */
@Mod(AuroraCompanionMod.MOD_ID)
public class AuroraCompanionMod {
    public static final String MOD_ID = "aurora_companion";

    public AuroraCompanionMod() {
        if (FMLEnvironment.dist.isClient()) {
            ClientCompanion.init();
        }
    }
}
