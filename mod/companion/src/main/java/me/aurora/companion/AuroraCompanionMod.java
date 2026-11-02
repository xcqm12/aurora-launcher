package me.aurora.companion;

import net.fabricmc.api.ModInitializer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Aurora Companion 只在客户端工作（environment=client），主入口保持空实现。
 */
public class AuroraCompanionMod implements ModInitializer {
    public static final Logger LOGGER = LoggerFactory.getLogger("aurora-companion");

    @Override
    public void onInitialize() {
        LOGGER.info("Aurora Companion loaded (client entrypoint drives everything)");
    }
}
