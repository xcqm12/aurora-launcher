package me.aurora.companionexp;

import net.minecraft.network.syncher.SynchedEntityData;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.entity.ai.attributes.AttributeSupplier;
import net.minecraft.world.entity.ai.attributes.Attributes;
import net.minecraft.world.level.Level;
import net.minecraft.world.phys.Vec3;

/**
 * 小七的实体本体。只在客户端世界里存在（幽灵假玩家）：
 * - 覆盖 tick/aiStep：不跑任何原版 AI、不参与存档/物理，位置完全由
 *   {@link #setAnchor} 驱动（客户端每 tick 计算锚点）。
 * - 渲染见 {@code XiaoqiRenderer}（PlayerModel Slim + 专属皮肤纹理）。
 */
public class XiaoqiEntity extends Mob {
    /** 跟随模式：跟随 / 停下 / 待命。 */
    public enum Mode {
        FOLLOW("跟随"),
        STOP("待命");

        public final String label;

        Mode(String label) {
            this.label = label;
        }
    }

    private Mode mode = Mode.FOLLOW;
    /** 锚点由客户端逻辑每 tick 设置；实体只平滑趋近它。 */
    private Vec3 anchor = Vec3.ZERO;
    private boolean lookingAtPlayer = false;

    public XiaoqiEntity(EntityType<? extends XiaoqiEntity> type, Level level) {
        super(type, level);
        setNoGravity(true);      // 幽灵：不受重力，跟随锚点
        setSilent(true);         // 不发出声音
        setInvulnerable(true);   // 不受伤害
        setPersistenceRequired();// 客户端 tick 时避免被清理
    }

    public static AttributeSupplier.Builder createAttributes() {
        return Mob.createMobAttributes()
                .add(Attributes.MAX_HEALTH, 20.0)
                .add(Attributes.MOVEMENT_SPEED, 0.3);
    }

    /* ── 客户端控制接口 ─────────────────────────────────── */

    public Mode mode() {
        return mode;
    }

    public void setMode(Mode mode) {
        this.mode = mode;
    }

    public void setAnchor(Vec3 anchor) {
        this.anchor = anchor;
    }

    public void setLookingAtPlayer(boolean looking) {
        this.lookingAtPlayer = looking;
    }

    /* ── 实体行为：完全由锚点驱动 ───────────────────────── */

    @Override
    public void tick() {
        super.tick();
        // 平滑趋近锚点（每 tick 最多走 0.35 格，避免瞬移感）
        Vec3 target = anchor;
        Vec3 delta = target.subtract(position());
        if (delta.lengthSqr() > 0.0001) {
            Vec3 step = delta.length() > 0.35 ? delta.normalize().scale(0.35) : delta;
            setPos(getX() + step.x, getY() + step.y, getZ() + step.z);
        }
        // 朝向：默认朝视线方向（跟随玩家的面朝），look 模式朝玩家
        if (lookingAtPlayer) {
            faceToward(playerEyePos());
        }
    }

    private Vec3 playerEyePos() {
        var player = level().getNearestPlayer(this, 64.0);
        return player != null ? player.getEyePosition() : position().add(0, 1.6, 0);
    }

    private void faceToward(Vec3 target) {
        Vec3 look = target.subtract(position().add(0, 1.6, 0));
        double horizontal = Math.sqrt(look.x * look.x + look.z * look.z);
        setYRot((float) (Math.atan2(look.z, look.x) * 180 / Math.PI) - 90);
        setXRot((float) (-Math.atan2(look.y, horizontal) * 180 / Math.PI));
        yHeadRot = getYRot();
    }

    @Override
    protected void registerGoals() {
        // 不注册任何原版 AI
    }

    @Override
    public void aiStep() {
        // 覆盖：不做原版移动/重力/水/岩浆逻辑
    }

    @Override
    public boolean isPushable() {
        return false;
    }

    @Override
    public boolean canBeCollidedWith() {
        return false;
    }

    /* ── 存档 / 同步数据（空实现） ─────────────────────── */

    @Override
    protected void defineSynchedData(SynchedEntityData.Builder builder) {
        // 必须调 super：LivingEntity/Mob 的 Health 等基础同步字段在父类注册，
        // 漏掉会抛 "has not defined synched data value 8"
        super.defineSynchedData(builder);
    }

    @Override
    public void readAdditionalSaveData(net.minecraft.nbt.CompoundTag tag) {
    }

    @Override
    public void addAdditionalSaveData(net.minecraft.nbt.CompoundTag tag) {
    }
}
