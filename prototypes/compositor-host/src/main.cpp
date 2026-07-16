#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

#include <wayland-server-core.h>

#include <src/Compositor.hpp>
#include <src/event/EventBus.hpp>
#include <src/helpers/Monitor.hpp>
#include <src/managers/SeatManager.hpp>
#include <src/plugins/PluginAPI.hpp>
#include <src/protocols/core/Compositor.hpp>
#include <src/protocols/core/Output.hpp>
#include <src/protocols/types/SurfaceRole.hpp>
#include <src/render/Renderer.hpp>
#include <src/render/pass/SurfacePassElement.hpp>

#include "htm-shell-v1-server-protocol.h"

namespace {

    constexpr uint32_t ROOT_WIDTH            = 640;
    constexpr uint32_t ROOT_HEIGHT           = 160;
    constexpr uint32_t MAX_SURFACE_DIMENSION = 4096;
    constexpr auto     TOKEN_ENV             = "HTM_SHELL_PROBE_TOKEN";

    class CHtmShellHost;
    class CManagerBinding;
    class CShellRoot;

    UP<CHtmShellHost> g_host;

    class CProbeSurfaceRole final : public ISurfaceRole {
      public:
        eSurfaceRole role() override {
            return sc<eSurfaceRole>(0x7f);
        }
    };

    bool constantTimeEqual(const std::string& left, const char* right) {
        if (!right)
            return false;

        const size_t rightLength = std::strlen(right);
        const size_t longest     = std::max(left.size(), rightLength);
        size_t       difference  = left.size() ^ rightLength;

        for (size_t index = 0; index < longest; ++index) {
            const unsigned char leftByte  = index < left.size() ? sc<unsigned char>(left[index]) : 0;
            const unsigned char rightByte = index < rightLength ? sc<unsigned char>(right[index]) : 0;
            difference |= sc<size_t>(leftByte ^ rightByte);
        }

        return difference == 0;
    }

    class CManagerBinding {
      public:
        CManagerBinding(CHtmShellHost* host_, wl_client* client_, wl_resource* resource_) : host(host_), client(client_), resource(resource_) {}

        CHtmShellHost* host          = nullptr;
        wl_client*     client        = nullptr;
        wl_resource*   resource      = nullptr;
        bool           authenticated = false;
    };

    class CShellRoot {
      public:
        CShellRoot(CHtmShellHost* host_, CManagerBinding* manager_, wl_resource* resource_, SP<CWLSurfaceResource> surface_, PHLMONITOR monitor_);
        ~CShellRoot();

        void                   ackConfigure(uint32_t serial);
        void                   onPrecommit();
        void                   onCommit();
        void                   render(PHLMONITOR monitor, const Time::steady_tp& now);
        void                   deactivate();

        CHtmShellHost*         host     = nullptr;
        wl_client*             client   = nullptr;
        wl_resource*           resource = nullptr;
        SP<CWLSurfaceResource> surface;
        PHLMONITORREF          monitor;
        uint32_t               configureSerial = 0;
        uint32_t               logicalWidth    = 0;
        uint32_t               logicalHeight   = 0;
        Vector2D               outputLocalPosition;
        CBox                   globalLogicalBounds;
        bool                   configured  = false;
        bool                   active      = false;
        bool                   tearingDown = false;
        CHyprSignalListener    precommitListener;
        CHyprSignalListener    commitListener;
        CHyprSignalListener    surfaceDestroyListener;
    };

    class CHtmShellHost {
      public:
        explicit CHtmShellHost(std::string token_) : token(std::move(token_)) {
            wl_list_init(&controllerDestroy.link);
            controllerDestroy.notify = onControllerDestroy;
            global                   = wl_global_create(g_pCompositor->m_wlDisplay, &htm_shell_manager_v1_interface, 1, this, bindManager);
            if (!global)
                throw std::runtime_error("failed to register htm_shell_manager_v1");

            renderListener = Event::bus()->m_events.render.stage.listen([this](eRenderStage stage) {
                if (stage != RENDER_POST_WINDOWS || !g_pHyprRenderer)
                    return;

                const auto monitor = g_pHyprRenderer->m_renderData.pMonitor.lock();
                if (!monitor)
                    return;

                const auto now  = Time::steadyNow();
                const auto copy = roots;
                for (const auto root : copy) {
                    if (root)
                        root->render(monitor, now);
                }
            });
        }

        ~CHtmShellHost() {
            renderListener.reset();

            if (global) {
                wl_global_destroy(global);
                global = nullptr;
            }

            // The experimental surface-role object's vtable belongs to this plugin.
            // Disconnect the controller while the plugin is still mapped so all role
            // objects are destroyed before Hyprland unloads the shared library.
            if (controller) {
                const auto client = controller;
                wl_list_remove(&controllerDestroy.link);
                wl_list_init(&controllerDestroy.link);
                controller = nullptr;
                wl_client_destroy(client);
            }

            const auto rootsCopy = roots;
            for (const auto root : rootsCopy) {
                if (root && root->resource)
                    wl_resource_destroy(root->resource);
            }

            const auto managersCopy = managers;
            for (const auto manager : managersCopy) {
                if (manager && manager->resource)
                    wl_resource_destroy(manager->resource);
            }
        }

        void authorize(CManagerBinding* binding, const char* candidate) {
            if (!binding || !binding->resource)
                return;
            if (binding->authenticated) {
                wl_resource_post_error(binding->resource, HTM_SHELL_MANAGER_V1_ERROR_ALREADY_AUTHENTICATED, "manager is already authenticated");
                return;
            }
            if (controller) {
                wl_resource_post_error(binding->resource, HTM_SHELL_MANAGER_V1_ERROR_CONTROLLER_EXISTS, "an HTMShell controller already exists");
                return;
            }
            const size_t candidateLength = candidate ? std::strlen(candidate) : 0;
            if (candidateLength < 32 || candidateLength > 256 || !constantTimeEqual(token, candidate)) {
                wl_resource_post_error(binding->resource, HTM_SHELL_MANAGER_V1_ERROR_UNAUTHORIZED, "HTMShell authorization failed");
                return;
            }

            binding->authenticated = true;
            controller             = binding->client;
            wl_client_add_destroy_listener(controller, &controllerDestroy);
            htm_shell_manager_v1_send_capability(binding->resource, HTM_SHELL_MANAGER_V1_CAPABILITY_ROOT_OVERLAY);
            htm_shell_manager_v1_send_ready(binding->resource);
        }

        void createRoot(CManagerBinding* binding, uint32_t id, wl_resource* surfaceResource, wl_resource* outputResource, uint32_t role) {
            if (!binding)
                return;
            if (!binding->authenticated || binding->client != controller) {
                wl_resource_post_error(binding->resource, HTM_SHELL_MANAGER_V1_ERROR_UNAUTHORIZED, "authorization is required before creating a root");
                return;
            }
            if (role != HTM_SHELL_MANAGER_V1_ROLE_OVERLAY) {
                wl_resource_post_error(binding->resource, HTM_SHELL_MANAGER_V1_ERROR_INVALID_ROLE, "unsupported semantic shell-root role");
                return;
            }

            const auto surface = CWLSurfaceResource::fromResource(surfaceResource);
            const auto output  = CWLOutputResource::fromResource(outputResource);
            if (!surface || surface->client() != binding->client || !surface->good()) {
                wl_resource_post_error(binding->resource, HTM_SHELL_MANAGER_V1_ERROR_INVALID_SURFACE, "invalid wl_surface");
                return;
            }
            if (!output || output->client() != binding->client || !output->m_monitor || !g_pCompositor->monitorExists(output->m_monitor.lock())) {
                wl_resource_post_error(binding->resource, HTM_SHELL_MANAGER_V1_ERROR_INVALID_OUTPUT, "invalid wl_output");
                return;
            }
            if (!surface->m_role || surface->m_role->role() != SURFACE_ROLE_UNASSIGNED) {
                const uint32_t error =
                    surface->m_role && surface->m_role->role() == sc<eSurfaceRole>(0x7f) ? HTM_SHELL_MANAGER_V1_ERROR_DUPLICATE_ROOT : HTM_SHELL_MANAGER_V1_ERROR_INVALID_SURFACE;
                wl_resource_post_error(binding->resource, error, "wl_surface already has a role");
                return;
            }

            auto rootResource = wl_resource_create(binding->client, &htm_shell_root_v1_interface, wl_resource_get_version(binding->resource), id);
            if (!rootResource) {
                wl_client_post_no_memory(binding->client);
                return;
            }

            surface->m_role = makeShared<CProbeSurfaceRole>();
            auto root       = new CShellRoot(this, binding, rootResource, surface, output->m_monitor.lock());
            roots.push_back(root);
            wl_resource_set_implementation(rootResource, &rootImplementation, root, destroyRootResource);
            root->configureSerial = wl_display_next_serial(g_pCompositor->m_wlDisplay);
            htm_shell_root_v1_send_configure(rootResource, root->configureSerial, root->logicalWidth, root->logicalHeight);
        }

        void destroyManager(CManagerBinding* binding) {
            if (!binding)
                return;
            std::erase(managers, binding);
            binding->resource = nullptr;
            delete binding;
        }

        void destroyRoot(CShellRoot* root) {
            if (!root || root->tearingDown)
                return;
            root->tearingDown = true;
            root->deactivate();
            std::erase(roots, root);
            root->resource = nullptr;
            delete root;
        }

        void onClientDestroyed(wl_client* client) {
            const auto rootsCopy = roots;
            for (const auto root : rootsCopy) {
                if (root && root->client == client)
                    root->deactivate();
            }
            controller = nullptr;
            wl_list_remove(&controllerDestroy.link);
            wl_list_init(&controllerDestroy.link);
        }

        static void bindManager(wl_client* client, void* data, uint32_t version, uint32_t id) {
            auto host     = static_cast<CHtmShellHost*>(data);
            auto resource = wl_resource_create(client, &htm_shell_manager_v1_interface, std::min(version, 1U), id);
            if (!resource) {
                wl_client_post_no_memory(client);
                return;
            }

            auto binding = new CManagerBinding(host, client, resource);
            host->managers.push_back(binding);
            wl_resource_set_implementation(resource, &managerImplementation, binding, destroyManagerResource);
        }

        static void authenticateRequest(wl_client*, wl_resource* resource, const char* token) {
            const auto binding = static_cast<CManagerBinding*>(wl_resource_get_user_data(resource));
            if (binding)
                binding->host->authorize(binding, token);
        }

        static void getRootRequest(wl_client*, wl_resource* resource, uint32_t id, wl_resource* surface, wl_resource* output, uint32_t role) {
            const auto binding = static_cast<CManagerBinding*>(wl_resource_get_user_data(resource));
            if (binding)
                binding->host->createRoot(binding, id, surface, output, role);
        }

        static void destroyManagerRequest(wl_client*, wl_resource* resource) {
            wl_resource_destroy(resource);
        }

        static void destroyManagerResource(wl_resource* resource) {
            const auto binding = static_cast<CManagerBinding*>(wl_resource_get_user_data(resource));
            if (binding)
                binding->host->destroyManager(binding);
        }

        static void ackConfigureRequest(wl_client*, wl_resource* resource, uint32_t serial) {
            const auto root = static_cast<CShellRoot*>(wl_resource_get_user_data(resource));
            if (root)
                root->ackConfigure(serial);
        }

        static void destroyRootRequest(wl_client*, wl_resource* resource) {
            wl_resource_destroy(resource);
        }

        static void destroyRootResource(wl_resource* resource) {
            const auto root = static_cast<CShellRoot*>(wl_resource_get_user_data(resource));
            if (root)
                root->host->destroyRoot(root);
        }

        static void onControllerDestroy(wl_listener* listener, void* data) {
            CHtmShellHost* host = nullptr;
            host                = wl_container_of(listener, host, controllerDestroy);
            const auto client   = static_cast<wl_client*>(data);
            host->onClientDestroyed(client);
        }

        static constexpr struct htm_shell_manager_v1_interface managerImplementation = {
            .authenticate = authenticateRequest,
            .get_root     = getRootRequest,
            .destroy      = destroyManagerRequest,
        };

        static constexpr struct htm_shell_root_v1_interface rootImplementation = {
            .ack_configure = ackConfigureRequest,
            .destroy       = destroyRootRequest,
        };

        std::string                   token;
        wl_global*                    global     = nullptr;
        wl_client*                    controller = nullptr;
        wl_listener                   controllerDestroy;
        std::vector<CManagerBinding*> managers;
        std::vector<CShellRoot*>      roots;
        CHyprSignalListener           renderListener;
    };

    CShellRoot::CShellRoot(CHtmShellHost* host_, CManagerBinding* manager_, wl_resource* resource_, SP<CWLSurfaceResource> surface_, PHLMONITOR monitor_) :
        host(host_), client(manager_->client), resource(resource_), surface(std::move(surface_)), monitor(monitor_) {
        logicalWidth        = sc<uint32_t>(std::min<double>(ROOT_WIDTH, monitor_->m_size.x));
        logicalHeight       = sc<uint32_t>(std::min<double>(ROOT_HEIGHT, monitor_->m_size.y));
        outputLocalPosition = {
            std::max(0.0, (monitor_->m_size.x - logicalWidth) / 2.0),
            std::min(24.0, std::max(0.0, monitor_->m_size.y - logicalHeight)),
        };
        globalLogicalBounds = CBox{monitor_->m_position + outputLocalPosition, Vector2D{sc<double>(logicalWidth), sc<double>(logicalHeight)}};

        surface->enter(monitor_);
        surface->sendPreferredScale(std::max(1, sc<int32_t>(std::ceil(monitor_->m_scale))));
        precommitListener      = surface->m_events.precommit.listen([this] { onPrecommit(); });
        commitListener         = surface->m_events.commit.listen([this] { onCommit(); });
        surfaceDestroyListener = surface->m_events.destroy.listen([this] {
            if (resource)
                wl_resource_destroy(resource);
        });
    }

    CShellRoot::~CShellRoot() {
        precommitListener.reset();
        commitListener.reset();
        surfaceDestroyListener.reset();
    }

    void CShellRoot::ackConfigure(uint32_t serial) {
        if (!resource)
            return;
        if (serial != configureSerial || configured) {
            wl_resource_post_error(resource, HTM_SHELL_ROOT_V1_ERROR_INVALID_ACK, "unknown, stale, or duplicate configure serial");
            return;
        }
        configured = true;
    }

    void CShellRoot::onPrecommit() {
        if (!surface || !resource)
            return;
        if (surface->m_pending.updated.bits.buffer && surface->m_pending.buffer && !configured) {
            surface->m_pending.rejected = true;
            wl_resource_post_error(resource, HTM_SHELL_ROOT_V1_ERROR_BUFFER_BEFORE_ACK, "buffer committed before initial configure acknowledgement");
        }
    }

    void CShellRoot::onCommit() {
        if (!surface || tearingDown)
            return;

        const auto size       = surface->m_current.size;
        const auto bufferSize = surface->m_current.bufferSize;
        if (size.x < 0 || size.y < 0 || size.x > MAX_SURFACE_DIMENSION || size.y > MAX_SURFACE_DIMENSION || bufferSize.x < 0 || bufferSize.y < 0 ||
            bufferSize.x > MAX_SURFACE_DIMENSION || bufferSize.y > MAX_SURFACE_DIMENSION) {
            if (resource)
                wl_resource_post_error(resource, HTM_SHELL_ROOT_V1_ERROR_INVALID_SIZE, "committed surface dimensions exceed probe limits");
            return;
        }

        const bool nowActive = configured && surface->m_current.texture && size.x > 0 && size.y > 0;
        if (nowActive && !active)
            surface->map();
        else if (!nowActive && active)
            surface->unmap();
        active = nowActive;

        if (g_pHyprRenderer)
            g_pHyprRenderer->damageBox(globalLogicalBounds);
    }

    void CShellRoot::render(PHLMONITOR renderMonitor, const Time::steady_tp& now) {
        const auto ownMonitor = monitor.lock();
        if (!active || !surface || !ownMonitor || ownMonitor.get() != renderMonitor.get() || !surface->m_current.texture)
            return;

        CSurfacePassElement::SRenderData data;
        data.pMonitor    = ownMonitor;
        data.when        = now;
        data.pos         = globalLogicalBounds.pos();
        data.localPos    = {};
        data.surface     = surface;
        data.texture     = surface->m_current.texture;
        data.mainSurface = true;
        data.w           = logicalWidth;
        data.h           = logicalHeight;
        data.dontRound   = true;
        data.alpha       = 1.F;
        data.fadeAlpha   = 1.F;
        data.blur        = false;
        data.clipBox     = CBox{outputLocalPosition * ownMonitor->m_scale, Vector2D{sc<double>(logicalWidth), sc<double>(logicalHeight)} * ownMonitor->m_scale};

        g_pHyprRenderer->m_renderPass.add(makeUnique<CSurfacePassElement>(data));
        surface->frame(now);
    }

    void CShellRoot::deactivate() {
        if (!surface)
            return;

        active = false;
        if (g_pSeatManager && g_pSeatManager->m_state.pointerFocus.lock() == surface)
            g_pSeatManager->setPointerFocus(nullptr, {});
        surface->unmap();
        if (g_pHyprRenderer)
            g_pHyprRenderer->damageBox(globalLogicalBounds);

        precommitListener.reset();
        commitListener.reset();
        surfaceDestroyListener.reset();
        surface.reset();
    }

} // namespace

APICALL EXPORT std::string PLUGIN_API_VERSION() {
    return HYPRLAND_API_VERSION;
}

APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE) {
    const char* token = std::getenv(TOKEN_ENV);
    if (!token || std::strlen(token) < 32 || std::strlen(token) > 256) {
        Log::logger->log(Log::ERR, "HTMShell compositor-host probe requires a 32-to-256-byte inherited capability");
        return {"htm-shell-compositor-host", "HTMShell universal contract feasibility host (inactive)", "CoastLineSec", "0.0.0"};
    }

    try {
        g_host = makeUnique<CHtmShellHost>(std::string{token});
    } catch (const std::exception& error) {
        Log::logger->log(Log::ERR, "HTMShell compositor-host probe initialization failed: {}", error.what());
        g_host.reset();
    }

    return {"htm-shell-compositor-host", "HTMShell universal contract feasibility host", "CoastLineSec", "0.0.0"};
}

APICALL EXPORT void PLUGIN_EXIT() {
    g_host.reset();
}
