#![allow(dead_code, unused_variables, unused_parens, clippy::all)]

macro_rules! argon_module {
    ($name:ident) => {
        pub mod $name {
            include!(concat!(
                env!("OUT_DIR"),
                concat!("/", stringify!($name), ".rs")
            ));
        }
    };
}

argon_module!(app_approval_bar);
argon_module!(app_button);
argon_module!(app_chat_view);
argon_module!(app_dialog);
argon_module!(app_message);
argon_module!(app_message_actions);
argon_module!(app_message_scroller);
argon_module!(app_model_picker);
argon_module!(app_prompt_input);
argon_module!(app_quiz_bar);
argon_module!(app_sidebar);
argon_module!(app_spoiler);
argon_module!(app_spinner);
argon_module!(app_text_input);
argon_module!(auth_form);
argon_module!(auto_markdown);
argon_module!(app_tool_activity);
argon_module!(app_tool_cluster);
argon_module!(app_work_group);
argon_module!(app_tooltip);
argon_module!(file_state);
argon_module!(settings);
argon_module!(side_panel);
argon_module!(ui);
argon_module!(timeline);
argon_module!(model_option);
argon_module!(thread_view);
argon_module!(threads_page_view);
argon_module!(shell_page_view);

argon_module!(archive);
argon_module!(arrow_up);
argon_module!(bot_message_square);
argon_module!(chevron_down);
argon_module!(chevron_left);
argon_module!(chevron_right);
argon_module!(chevron_up);
argon_module!(check);
argon_module!(copy);
argon_module!(eye);
argon_module!(file);
argon_module!(files);
argon_module!(folder);
argon_module!(mic);
argon_module!(panel_left_close);
argon_module!(panel_left_open);
argon_module!(panel_right);
argon_module!(plus);
argon_module!(settings_horizontal);
argon_module!(stop);
argon_module!(terminal);
argon_module!(trash_2);
argon_module!(upload);
argon_module!(workflow);
argon_module!(x);
