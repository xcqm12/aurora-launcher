package me.aurora.companionexp;

import net.minecraft.client.model.PlayerModel;
import net.minecraft.client.model.geom.ModelLayers;
import net.minecraft.client.renderer.entity.EntityRendererProvider;
import net.minecraft.client.renderer.entity.LivingEntityRenderer;
import net.minecraft.client.renderer.entity.state.PlayerRenderState;
import net.minecraft.resources.ResourceLocation;

/**
 * 小七的渲染器：玩家 Slim 模型 + 专属皮肤纹理。
 * 皮肤 = brand/skin-xiaoqi-128.png（128×128 双层），作为实体纹理直接绑定，
 * 完全不依赖皮肤服务器 / 玩家档案 / 正版 ID。
 */
public class XiaoqiRenderer
        extends LivingEntityRenderer<XiaoqiEntity, PlayerRenderState, PlayerModel> {

    private static final ResourceLocation SKIN =
            ResourceLocation.fromNamespaceAndPath(XiaoqiMod.MOD_ID, "textures/entity/xiaoqi.png");

    public XiaoqiRenderer(EntityRendererProvider.Context ctx) {
        super(ctx, new PlayerModel(ctx.bakeLayer(ModelLayers.PLAYER_SLIM), true), 0.5f);
    }

    @Override
    public PlayerRenderState createRenderState() {
        return new PlayerRenderState();
    }

    @Override
    public void extractRenderState(XiaoqiEntity entity, PlayerRenderState state, float partialTick) {
        super.extractRenderState(entity, state, partialTick);
        state.name = "小七";
    }

    @Override
    public ResourceLocation getTextureLocation(PlayerRenderState state) {
        return SKIN;
    }
}
