package me.aurora.companionexp;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.message.v1.ClientSendMessageEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.rendering.v1.EntityRendererRegistry;
import net.minecraft.client.Minecraft;
import net.minecraft.network.chat.Component;
import net.minecraft.world.phys.Vec3;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.concurrent.CompletableFuture;

/**
 * 小七客户端逻辑（实验项目）：
 * 1. 进世界自动生成幽灵假玩家（客户端专属，服务端不可见，任意服务器可用）
 * 2. 跟随玩家（平滑锚点趋近），支持 #ai 指令切换跟随/待命/过来/看我
 * 3. 聊天走启动器本地 AI 桥（127.0.0.1:<port>，Bearer token），
 *    由 Rust 侧决定走 LLM 还是规则兜底
 */
public class XiaoqiClient implements ClientModInitializer {
    private static final Gson GSON = new Gson();
    private static final Logger LOGGER = LoggerFactory.getLogger("xiaoqi-companion");
    private static final String PREFIX = "#ai";

    private final HttpClient http = HttpClient.newBuilder()
            .connectTimeout(Duration.ofSeconds(5))
            .build();

    private String port = "";
    private String token = "";
    private String playerName = "Player";
    private boolean bridge = false;
    private boolean inWorld = false;
    private boolean awaitingReply = false;
    private int tickCounter = 0;
    private XiaoqiEntity companion;
    private Vec3 anchor = Vec3.ZERO;
    /** look 指令的剩余 tick（>0 时持续看向玩家）。 */
    private int lookTicks = 0;

    @Override
    public void onInitializeClient() {
        EntityRendererRegistry.register(XiaoqiMod.XIAOQI, XiaoqiRenderer::new);

        // 启动器注入：-Dmcne.ai.port/token/player（兼容旧的 aurora.ai.*）
        port = System.getProperty("mcne.ai.port", System.getProperty("aurora.ai.port", ""));
        token = System.getProperty("mcne.ai.token", System.getProperty("aurora.ai.token", ""));
        playerName = System.getProperty("mcne.ai.player", System.getProperty("aurora.ai.player", "Player"));
        bridge = !port.isEmpty() && !token.isEmpty();
        if (!bridge) {
            LOGGER.info("AI 桥未注入（-Dmcne.ai.port/token 缺失）：小七只能执行本地跟随指令");
        }

        ClientPlayConnectionEvents.JOIN.register((handler, sender, client) -> {
            inWorld = true;
            tickCounter = 0;
            companion = null; // 下次 tick 重新生成
            if (bridge) {
                JsonObject body = new JsonObject();
                body.addProperty("player", playerName);
                post("/hello", body).thenAccept(reply -> {
                    String greeting = stringField(reply, "greeting");
                    if (!greeting.isEmpty()) {
                        say(greeting);
                    }
                });
            } else {
                say("§7(小七已就位——启动器未开启 AI 桥，可用 #ai 跟随 / #ai 停下 指挥)");
            }
        });

        ClientPlayConnectionEvents.DISCONNECT.register((handler, client) -> {
            inWorld = false;
            removeCompanion();
        });

        ClientTickEvents.END_CLIENT_TICK.register(this::onTick);

        // #ai 指令：本地规则优先（跟随/停下/过来/看我），其余转 AI 桥
        ClientSendMessageEvents.ALLOW_CHAT.register(message -> {
            if (inWorld && message.startsWith(PREFIX)) {
                String question = message.substring(PREFIX.length()).trim();
                if (!question.isEmpty()) {
                    handleCommand(question);
                }
                return false;
            }
            return true;
        });

        LOGGER.info("Xiaoqi companion ready: bridge={} player={}", bridge, playerName);
    }

    /* ── 每 tick：维持实体 + 跟随锚点 ───────────────────── */

    private void onTick(Minecraft client) {
        if (!inWorld || client.player == null || client.level == null) {
            return;
        }
        tickCounter++;
        if (companion == null || companion.isRemoved() || companion.level() != client.level) {
            spawnCompanion(client);
            return;
        }

        var player = client.player;
        if (companion.mode() == XiaoqiEntity.Mode.FOLLOW) {
            // 锚点：玩家身后 2.0 格、右肩侧 0.9 格，脚底与玩家同高
            double yaw = Math.toRadians(player.getYRot());
            Vec3 back = new Vec3(-Math.sin(yaw), 0, Math.cos(yaw)).scale(-2.0);
            Vec3 right = new Vec3(Math.cos(yaw), 0, Math.sin(yaw)).scale(0.9);
            anchor = player.position().add(back).add(right);
        }
        companion.setAnchor(anchor);
        if (lookTicks > 0) {
            lookTicks--;
            companion.setLookingAtPlayer(true);
        } else {
            companion.setLookingAtPlayer(false);
        }
    }

    private void spawnCompanion(Minecraft client) {
        var player = client.player;
        var level = client.level;
        if (player == null || level == null) {
            return;
        }
        XiaoqiEntity entity = XiaoqiMod.XIAOQI.create(level, net.minecraft.world.entity.EntitySpawnReason.COMMAND);
        if (entity == null) {
            LOGGER.warn("小七生成失败：实体类型 create 返回 null");
            return;
        }
        anchor = player.position();
        entity.setAnchor(anchor);
        entity.setPos(player.getX(), player.getY(), player.getZ());
        entity.setYRot(player.getYRot());
        entity.setCustomName(Component.literal("§d小七"));
        entity.setCustomNameVisible(true);
        level.addEntity(entity);
        companion = entity;
        LOGGER.info("小七已生成于 {}", player.position());
    }

    private void removeCompanion() {
        if (companion != null) {
            companion.discard();
            companion = null;
        }
    }

    /* ── 聊天指令 ───────────────────────────────────────── */

    private void handleCommand(String text) {
        String lower = text.toLowerCase();
        // 本地规则（立即生效，无需 AI 桥）
        if (lower.contains("跟随") || lower.contains("跟着") || lower.contains("跟上") || lower.contains("follow")) {
            setMode(XiaoqiEntity.Mode.FOLLOW);
            say("好，跟着你。");
            return;
        }
        if (lower.contains("停下") || lower.contains("待命") || lower.contains("stop") || lower.contains("stay")) {
            setMode(XiaoqiEntity.Mode.STOP);
            say("好，我在这儿等你。");
            return;
        }
        if (lower.contains("过来") || lower.contains("come")) {
            var player = Minecraft.getInstance().player;
            if (player != null && companion != null) {
                anchor = player.position();
                companion.setPos(player.getX(), player.getY(), player.getZ());
            }
            say("来啦。");
            return;
        }
        if (lower.contains("看我") || lower.contains("look")) {
            lookTicks = 100;
            say("嗯，在看你。");
            return;
        }
        // 其余交给 AI 桥（LLM / 规则兜底在 Rust 侧）
        if (!bridge) {
            say("（启动器未开启 AI，我只能执行：#ai 跟随 / 停下 / 过来 / 看我）");
            return;
        }
        ask(text);
    }

    private void setMode(XiaoqiEntity.Mode mode) {
        if (companion != null) {
            companion.setMode(mode);
        }
    }

    private void ask(String question) {
        if (awaitingReply) {
            say("（小七还在想上一句，稍等…）");
            return;
        }
        awaitingReply = true;
        say("（小七思考中…）");
        JsonObject body = new JsonObject();
        body.addProperty("player", playerName);
        body.addProperty("message", question);
        post("/chat", body).thenAccept(reply -> {
            String answer = stringField(reply, "reply");
            String action = stringField(reply, "action");
            applyAction(action);
            say(answer.isEmpty() ? "（小七走神了，再问一次试试）" : answer);
            awaitingReply = false;
        }).exceptionally(error -> {
            say("（联系不上 AI：检查启动器设置里的 AI 开关与 Key）");
            awaitingReply = false;
            return null;
        });
    }

    private void applyAction(String action) {
        switch (action) {
            case "follow" -> setMode(XiaoqiEntity.Mode.FOLLOW);
            case "stop" -> setMode(XiaoqiEntity.Mode.STOP);
            case "come" -> {
                var player = Minecraft.getInstance().player;
                if (player != null && companion != null) {
                    anchor = player.position();
                    companion.setPos(player.getX(), player.getY(), player.getZ());
                }
            }
            case "look" -> lookTicks = 100;
            default -> {
            }
        }
    }

    private CompletableFuture<JsonObject> post(String path, JsonObject body) {
        HttpRequest request;
        try {
            request = HttpRequest.newBuilder()
                    .uri(URI.create("http://127.0.0.1:" + port + path))
                    .timeout(Duration.ofSeconds(45))
                    .header("Authorization", "Bearer " + token)
                    .header("Content-Type", "application/json")
                    .POST(HttpRequest.BodyPublishers.ofString(GSON.toJson(body)))
                    .build();
        } catch (Exception error) {
            return CompletableFuture.failedFuture(error);
        }
        return http.sendAsync(request, HttpResponse.BodyHandlers.ofString())
                .thenApply(response -> GSON.fromJson(response.body(), JsonObject.class))
                .exceptionally(error -> {
                    LOGGER.warn("AI bridge request failed", error);
                    return new JsonObject();
                });
    }

    private void say(String message) {
        Minecraft client = Minecraft.getInstance();
        if (client != null && client.gui != null) {
            client.gui.getChat().addMessage(Component.literal("§d[小七] §f" + message));
        }
    }

    private static String stringField(JsonObject object, String field) {
        if (object == null) {
            return "";
        }
        return object.has(field) && !object.get(field).isJsonNull()
                ? object.get(field).getAsString() : "";
    }
}
