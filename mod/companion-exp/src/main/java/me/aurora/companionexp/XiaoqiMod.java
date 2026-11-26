package me.aurora.companionexp;

import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.object.builder.v1.entity.FabricDefaultAttributeRegistry;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.ResourceKey;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.MobCategory;

/**
 * 小七伙伴模组（实验项目 mc-native-exp）——公共侧：
 * 只注册实体类型与属性。实体本身只在客户端生成（幽灵假玩家），
 * 服务端永远看不到它，所以任何服务器都能用。
 */
public class XiaoqiMod implements ModInitializer {
    public static final String MOD_ID = "mcne";
    public static final ResourceLocation XIAOQI_ID = ResourceLocation.fromNamespaceAndPath(MOD_ID, "xiaoqi");

    public static final EntityType<XiaoqiEntity> XIAOQI = Registry.register(
            BuiltInRegistries.ENTITY_TYPE,
            XIAOQI_ID,
            EntityType.Builder.<XiaoqiEntity>of(XiaoqiEntity::new, MobCategory.MISC)
                    .sized(0.6f, 1.85f)
                    .clientTrackingRange(10)
                    .build(ResourceKey.create(Registries.ENTITY_TYPE, XIAOQI_ID)));

    @Override
    public void onInitialize() {
        FabricDefaultAttributeRegistry.register(XIAOQI, XiaoqiEntity.createAttributes());
    }
}
