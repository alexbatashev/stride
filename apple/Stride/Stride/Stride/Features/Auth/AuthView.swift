import ComposableArchitecture
import SwiftUI

struct AuthView: View {
    @Bindable var store: StoreOf<AuthFeature>
    @FocusState private var focus: Field?

    private enum Field {
        case server, username, password
    }

    var body: some View {
        ZStack {
            backdrop
            card
                .frame(maxWidth: 400)
                .padding(Metrics.gutter)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var backdrop: some View {
        LinearGradient(
            colors: [
                Color.accentColor.opacity(0.30),
                Color.accentColor.opacity(0.05),
                Color.clear
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
        .ignoresSafeArea()
        .background(.background)
    }

    private var card: some View {
        VStack(spacing: 22) {
            header
            modePicker
            fields
            if let error = store.errorMessage {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .transition(.opacity)
            }
            submitButton
        }
        .padding(28)
        .glassEffect(.regular, in: .rect(cornerRadius: 28))
        .overlay(
            RoundedRectangle(cornerRadius: 28).strokeBorder(Color.hairline)
        )
        .animation(.snappy, value: store.errorMessage)
        .animation(.snappy, value: store.mode)
    }

    private var header: some View {
        VStack(spacing: 8) {
            Image(systemName: "sparkles")
                .font(.system(size: 34, weight: .semibold))
                .foregroundStyle(.tint)
                .padding(18)
                .glassEffect(.regular.tint(.accentColor), in: .circle)
            Text("S.T.R.I.D.E.")
                .font(.largeTitle.bold())
            Text(store.mode == .login ? "Sign in to your assistant" : "Create your account")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
    }

    private var modePicker: some View {
        Picker("Mode", selection: $store.mode) {
            Text("Sign In").tag(AuthFeature.State.Mode.login)
            Text("Create Account").tag(AuthFeature.State.Mode.register)
        }
        .pickerStyle(.segmented)
    }

    private var fields: some View {
        VStack(spacing: 12) {
            field(title: "Server", systemImage: "server.rack") {
                TextField("stride.example.com", text: $store.serverURL)
                    .textContentType(.URL)
                    .focused($focus, equals: .server)
                    #if os(iOS) || os(visionOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
                    .autocorrectionDisabled()
                    .submitLabel(.next)
                    .onSubmit { focus = .username }
            }
            field(title: "Username", systemImage: "person") {
                TextField("username", text: $store.username)
                    .textContentType(.username)
                    .focused($focus, equals: .username)
                    #if os(iOS) || os(visionOS)
                    .textInputAutocapitalization(.never)
                    #endif
                    .autocorrectionDisabled()
                    .submitLabel(.next)
                    .onSubmit { focus = .password }
            }
            field(title: "Password", systemImage: "lock") {
                SecureField("password", text: $store.password)
                    .textContentType(.password)
                    .focused($focus, equals: .password)
                    .submitLabel(.go)
                    .onSubmit { store.send(.submitTapped) }
            }
        }
    }

    private func field<Content: View>(
        title: String,
        systemImage: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        HStack(spacing: 12) {
            Image(systemName: systemImage)
                .foregroundStyle(.secondary)
                .frame(width: 22)
            content()
                .textFieldStyle(.plain)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(Color.subtleFill, in: .rect(cornerRadius: 14))
    }

    private var submitButton: some View {
        Button {
            store.send(.submitTapped)
        } label: {
            HStack {
                if store.isSubmitting {
                    ProgressView().controlSize(.small)
                }
                Text(store.mode == .login ? "Sign In" : "Create Account")
                    .fontWeight(.semibold)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 6)
        }
        .buttonStyle(.glassProminent)
        .controlSize(.large)
        .disabled(!store.canSubmit)
    }
}
