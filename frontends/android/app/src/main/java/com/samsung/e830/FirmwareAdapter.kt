package com.samsung.e830

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.ImageButton
import android.widget.PopupMenu
import android.widget.TextView
import androidx.recyclerview.widget.RecyclerView

internal class FirmwareAdapter(
    private val onRun: (FirmwareEntry) -> Unit,
    private val onSelectOneNand: (FirmwareEntry) -> Unit,
    private val onUseErasedFlash: (FirmwareEntry) -> Unit,
    private val onRemove: (FirmwareEntry) -> Unit,
) : RecyclerView.Adapter<FirmwareAdapter.ViewHolder>() {
    private var items: List<FirmwareEntry> = emptyList()

    fun submit(entries: List<FirmwareEntry>) {
        items = entries
        notifyDataSetChanged()
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder =
        ViewHolder(LayoutInflater.from(parent.context).inflate(R.layout.item_firmware, parent, false))

    override fun onBindViewHolder(holder: ViewHolder, position: Int) = holder.bind(items[position])

    override fun getItemCount(): Int = items.size

    inner class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
        private val title: TextView = view.findViewById(R.id.firmware_title)
        private val subtitle: TextView = view.findViewById(R.id.firmware_subtitle)
        private val more: ImageButton = view.findViewById(R.id.btn_more)

        fun bind(entry: FirmwareEntry) {
            title.text = entry.name
            subtitle.setText(if (entry.onenand == null) R.string.nand_missing else R.string.nand_present)
            itemView.setOnClickListener { onRun(entry) }
            itemView.findViewById<View>(R.id.btn_run).setOnClickListener { onRun(entry) }
            more.setOnClickListener { showMenu(it, entry) }
        }

        private fun showMenu(anchor: View, entry: FirmwareEntry) {
            val popup = PopupMenu(anchor.context, anchor)
            popup.menu.add(0, ACTION_SELECT_NAND, 0, R.string.nand_select)
            if (entry.onenand != null) {
                popup.menu.add(0, ACTION_USE_ERASED, 1, R.string.nand_auto)
            }
            popup.menu.add(0, ACTION_REMOVE, 2, R.string.remove_firmware)
            popup.setOnMenuItemClickListener { item ->
                when (item.itemId) {
                    ACTION_SELECT_NAND -> onSelectOneNand(entry)
                    ACTION_USE_ERASED -> onUseErasedFlash(entry)
                    ACTION_REMOVE -> onRemove(entry)
                }
                true
            }
            popup.show()
        }
    }

    private companion object {
        const val ACTION_SELECT_NAND = 1
        const val ACTION_USE_ERASED = 2
        const val ACTION_REMOVE = 3
    }
}
